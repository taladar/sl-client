---
id: viewer-flexi-prim-picking
title: Pick flexi prims against their simulated geometry
topic: viewer
status: done
origin: follow-up from viewer-object-context-menu (2026-07-21)
refs: [viewer-object-context-menu, viewer-p32-1]
---

Context: [context/viewer.md](../context/viewer.md).

The world object pick (right-click context menu, and the left-click touch it
shares its ray walk with) casts `MeshRayCast` against mesh assets. A **flexi
prim**'s simulation (`flexi.rs`) rewrites its faces' `ATTRIBUTE_POSITION`
in-place every frame, so the narrow-phase triangles are current — but the
broad-phase `Aabb` on the face entity is computed **once at spawn** from the
undeformed shape and never refreshed. A flexi bent or swaying outside its
original bounds is unpickable there (the ray-cast's broad phase culls it), and
a click inside the stale box but outside the current geometry correctly
misses, so the failure reads as "flexi objects are hard to right-click /
touch".

Fix: keep the pickable bounds in step with the simulation — either update the
face entities' `Aabb` from the simulated positions (cheap: the chain solver
already knows the node extents; grow, don't recompute per-vertex), or give
flexi faces the same treatment skinned avatar meshes got (a fitted collider
tested explicitly — see `avatar_pick.rs` — though a refreshed `Aabb` is far
simpler here since the deformed vertices genuinely live in the mesh asset).

Verify with a long, soft flexi (the classic tail / cape case) bent well away
from its rest pose: touch and the object pie must both resolve on the *drawn*
geometry, and not on the empty air where the rest pose was.

## Done (2026-07-22)

The real defect was one step worse than the stale-`Aabb` guess above:
`build_flexi_faces` put **`NoFrustumCulling` on every flexi face**, and
`calculate_bounds` skips such entities entirely — so flexi faces never had an
`Aabb` *at all*, and `MeshRayCast` (which reads the `Aabb` non-optionally)
could never hit a flexi prim, bent or straight. Every flexi was silently
untouchable and un-menu-able, and also never frustum-culled.

Fix: drop the opt-out. Bevy 0.19's `calculate_bounds` has an
`AssetChanged<Mesh3d>` branch that *refreshes* an existing `Aabb` whenever the
mesh asset is mutated — and `simulate_flexi` rewrites the face positions
through `Assets::get_mut` every frame, which is exactly what that branch
watches. A flexi face is therefore an ordinary `Aabb`-managed entity whose
bounds track the *bent* geometry each simulated frame: picking (left-click
touch and the object pie share the ray walk) hits the flexi where it is
drawn, and frustum culling becomes correct instead of disabled. Unlike the
skinned-mesh case (GPU deformation that never exists in the mesh data, where
the opt-out is legitimate), a flexi's deformed vertices genuinely live in the
asset, so the refreshed bounds are exact — no fitted collider needed.

Tests: `objects::tests::flexi_faces_stay_aabb_managed` runs the real
`build_flexi_faces` and pins that no face opts out of `Aabb` management;
`flexi::tests::simulated_flexi_mesh_keeps_its_aabb_fresh` runs the full
pipeline (sim in `Update`, Bevy's own `calculate_bounds` in `PostUpdate`)
and asserts the `Aabb` is inserted from the deformed mesh and keeps
refreshing as the chain moves — pinning the `AssetChanged` mechanism a Bevy
upgrade could silently break. Live-verified against the in-world
`SLClientFlexi` / `SLClientFlexiH` prims on the local grid.
