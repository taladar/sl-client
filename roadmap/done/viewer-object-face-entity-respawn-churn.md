---
id: viewer-object-face-entity-respawn-churn
title: Object face entities are despawned + respawned same-frame (despawn-race churn)
topic: viewer
status: done
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

## Outcome (2026-09-04): rebuilds update in place

Fixed with [[viewer-prim-rebuild-drops-a-click]], which is the user-visible half
of this same churn. Every rebuild site in `objects.rs` now hands its existing
face entities to its own replacement geometry through a `FaceReuse` pool keyed
by Linden face id, so a re-tessellation, an LOD swap, a mesh or sculpt arrival
and a tree tier change all **update the face entity in place** — swapping its
mesh, material and `TextureFace` — instead of despawning it and spawning a
successor. Entities are only spawned for a face that had none and only despawned
for a face the new geometry does not have.

So the smell this task named is gone at its source: an entity queried as new is
no longer already dead, `PickId` and every other per-face component survive a
rebuild, and the entity allocations a rez used to burn are not spent. The four
`gpu_pick` `try_insert` guards stay — they cost nothing and still cover the
genuine remaining despawns (an object removed, a region purged).

The audit half is done too: the one-shot consumers of freshly-built faces
(`register_pbr_materials`, `apply_blinn_phong_hide`, `register_bump_faces`,
`register_legacy_materials`) were moved from `Added<PrimFaceEntity>` to
`Changed<PrimFaceEntity>`, since an in-place rebuild writes the marker without
adding it. `collect_pick_warm_set` already keyed on `Changed<Mesh3d>`, and
Bevy's own `calculate_bounds` refreshes a changed mesh handle's `Aabb` in the
same frame, so culling and the ray cast track the new geometry.

One path is deliberately left as it was: the rigged-attachment bind
(`rigged_attachments.rs`), which replaces a worn mesh's `face_entities` with
submeshes skinned to the wearer's skeleton. That is a **first** build, not a
rebuild — the object was pending on its asset and had no faces — so there is
nothing to hand back and no churn to remove. A texture edit on a worn rigged
mesh still routes through `apply_object`, whose pool despawns the old submeshes
before the bind rebuilds them, exactly as before.
