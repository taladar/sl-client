---
id: viewer-audit-collider-settle-treadmill
title: A mesh with no physics block rebuilds its collider and the BVH every frame forever
topic: viewer
status: done
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

## Fixed (2026-09-14)

**The mesh manager can now be asked.** `MeshPhysicsAvailability`
(`Ready` / `Absent` / `Pending`) and `MeshManager::physics_availability` say
which of the three things `physics()`'s `None` was standing for. `Absent` is
terminal — no physics block, a failed fetch, or the nil id — and is the answer
the collider builders were missing.

**A parked request is no longer a dropped one.** `request_physics` used to
early-return when the mesh capability was unknown, which was only survivable
because the caller re-asked every frame. It now holds the id in
`physics_parked`, `retry_pending` drains it once the cap is up (beside the
geometry `pending` it already drained), and `deferred_count` counts it — so the
`has_cap_url() == false` half of this bug goes with the rest.

**Both collider paths name what they wait on.** `StaticCollider::settled` and
`RefinedCollider::from_geometry` were each one bit covering two unrelated waits
— geometry that has not streamed in (which arrives, announced no other way, so
retrying is right) and a mesh standing on its visual geometry while its physics
fetches (where retrying is right only until that fetch *answers*). Both are now
a `ColliderWait`: `Nothing`, `Geometry`, or `MeshPhysics(key)`.
`geometry_collider_wait` picks one from the mesh's availability and whether
there were vertices; `collider_wait_due` decides whether a frame's retry could
change anything. A `MeshPhysics` wait re-queues **once**, when the fetch
answers, and settles either way.

Unit-verified, four tests: `meshes.rs` —
`physics_availability_tells_an_absent_block_from_a_pending_fetch`,
`a_physics_request_made_before_the_cap_is_parked_and_reissued`; `physics.rs` —
`a_mesh_with_no_physics_block_settles_on_its_visual_geometry` (the assertion
this task asked for) and
`only_a_wait_that_could_have_ended_re_queues_a_collider`.

One residue, strictly better than before and worth naming: a mesh whose physics
resolved *and* whose visual geometry then went away (a LOD swap despawning its
faces) re-gathers geometry each frame until faces return, because the no-job
path deliberately does not re-insert a collider it cannot improve. That costs a
`gather_object_geometry` under the 16-prim budget — no trimesh, no re-insert,
no BVH rebuild — where today's code pays the whole chain.
