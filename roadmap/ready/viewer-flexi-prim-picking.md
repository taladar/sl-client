---
id: viewer-flexi-prim-picking
title: Pick flexi prims against their simulated geometry
topic: viewer
status: ready
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
