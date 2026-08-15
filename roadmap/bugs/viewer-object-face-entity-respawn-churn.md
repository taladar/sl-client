---
id: viewer-object-face-entity-respawn-churn
title: Object face entities are despawned + respawned same-frame (despawn-race churn)
topic: viewer
status: bugs
origin: gpu_pick despawn-race panic during camera-collision live testing (2026-08-15)
refs: [viewer-physics-static-prim-colliders]
---

Context: [context/viewer.md](../context/viewer.md).

During an aditi rez the viewer panicked in
`gpu_pick::assign_object_face_pick_tags`: a deferred
`commands.entity(e).insert((PickId, MeshTag))` was applied to a face entity that
**another system had already despawned in the same frame** ("Entity despawned:
… index now has generation 1"). Mitigated for now by switching the four
`gpu_pick` tag-insert sites to `try_insert` (no-op on a despawned entity; the
replacement is tagged next frame).

But the underlying smell is the real bug: an entity queried as **new** this
frame (`Without<PickId>`, `With<Mesh3d>`) is **already despawned** by the time
deferred commands apply. That means an object's face entities are being
**despawned and respawned** on a re-tessellation / LOD swap / rebuild rather
than **updated in place** — the same anti-pattern the UI floaters hit (see the
"build once, update in place" fix and the never-hide-errors note). Churning
entities every rez frame also burns entity allocations, defeats
`Without<PickId>`-style one-shot work (everything is re-done for the new
generation), and creates despawn races that *any* system touching those entities
can trip on — `gpu_pick` is just the one that happened to panic.

## Investigate

- Which path despawns + respawns object face entities: `apply_prim_lod` /
  `apply_object_meshes` re-tessellation, the render-priority LOD swap, a shape
  change in `apply_object`? Find where the old face children are despawned and
  new ones spawned.
- Can it **update in place** instead — swap the `Mesh3d` handle (and material)
  on the existing face entities, spawning/despawning only when the face *count*
  changes? That keeps `PickId`, colliders, and other per-face state stable
  across a LOD swap.
- Audit other systems that insert/mutate on freshly-queried object faces for the
  same latent race (the static-collider path already uses `get_entity` /
  `try_insert`-style guards; others may not).
- If churn is genuinely unavoidable for a case, ensure every consumer uses a
  despawn-tolerant command (`try_insert`) rather than a bare `insert`.

Compare with the UI floater fix — "don't despawn+respawn per update; build once,
set values in place" — which addressed exactly this shape of churn for widgets.
