---
id: viewer-prim-rebuild-drops-a-click
title: A prim's re-tessellation frame swallows any click on it
topic: viewer
status: done
origin: found while writing the pie-target negatives (2026-08-31)
points: 3
refs: [viewer-world-pie-target-tests, viewer-cpu-pick-resolver]
---

Context: [context/testing.md](../context/testing.md).

When a prim's faces are rebuilt, the old face entities are despawned and
the new ones spawned, and for **one frame** the prim has no face entities
at all. A pick resolved on that frame hits nothing, so the click that
asked for it is silently dropped: no pie, no touch, no selection — and
nothing anywhere says why.

Measured in the fixture world ([[viewer-world-test-harness]]) with a
per-frame census of `With<MeshTag>` face entities. The fixture prim's
faces are entity `266v0` on frames 2–16, **no faces on frame 17**, and
entity `271v0` from frame 18 — a rebuild about ten frames after the
camera lands. A right-click whose release falls on frame 17 requests its
pick normally (the gesture guards all pass, the ray is aimed correctly at
the prim), and `resolve_cpu_picks` answers with `hit: None`. Frame 16 or
18: exactly one pie. This is not the CPU resolver's doing — the GPU
resolver rasterises the same absent geometry — and not the fixture's:
any LOD switch, mesh arrival or texture-driven re-mesh does the same
thing to a live prim.

The user-visible shape is a rare "the menu just didn't open" — one
dropped click in the frame a prim happens to re-mesh, most likely
while a region is still rezzing, which is exactly when a user clicks
around most.

Worth fixing at the rebuild, not at the pick: build the new faces before
despawning the old ones (or keep the old entities and swap their meshes,
which the material-rebind rules already push us toward), so a prim is
never *absent*, only momentarily stale. A pick that resolves to the
about-to-be-replaced face still names the right object, which is all the
menus need.

The two pie negatives ([[viewer-world-pie-target-tests]]) settle past
this frame deliberately and say so: a negative that landed on it would
pass for the wrong reason.

## Outcome (2026-09-04): fixed at the rebuild — a face keeps its entity

Every rebuild path in `sl-viewer-world-objects/src/objects.rs` now offers the
face entities an object already has to its own replacement geometry, through a
`FaceReuse` pool: the shape / texture re-tessellation in `apply_object`, the
mesh arrival and the mesh LOD swap in `apply_object_meshes`, both arms of
`apply_prim_lod` (prim and sculpt), the sculpt-map arrival in
`apply_object_sculpts`, and `apply_tree_lod`'s tier swap.

The pool is keyed by **Linden face id**, not by position in the build order: a
face entity's identity downstream *is* its face — its pick-registry slot records
`(object, face)` — so handing face 3's entity to face 4 would answer clicks with
the wrong face. Each replacement face re-describes the entity that drew it (new
mesh, new material, new `TextureFace`); only a face the new geometry no longer
produces is despawned, and only a face with no predecessor spawns.

That removes the window rather than shortening it. The entity keeps its `PickId`
and `MeshTag`, so it never stops being pickable — there is no longer a frame
with no face entities at all, and nothing that recorded a face entity id is left
holding a dead one ([[viewer-object-face-entity-respawn-churn]], fixed by the
same change).

The four one-shot registrars that keyed on `Added<PrimFaceEntity>` —
`register_pbr_materials`, `apply_blinn_phong_hide`, `register_bump_faces`,
`register_legacy_materials` — key on `Changed<PrimFaceEntity>` instead: a
rebuild *writes* the marker but only the first build *adds* it, and a
re-tessellated face must re-register against its new material handle.

Pinned by two tests:

- `a_rebuild_reuses_each_face_entity` (`objects.rs`) — on the real
  `apply_object` path, a texture edit hands every face back its own entity with
  a marker component another system attached still on it; the rebuilt face reads
  as **changed** (with `Added` in its place the same query returns nothing,
  which is the registrar regression the switch prevents); and a prim that
  becomes a tree keeps face 0's entity while the faces it no longer has are
  despawned.
- `a_prim_keeps_its_pick_tagged_faces_across_its_rebuild` (`world_test.rs`) —
  the per-frame census of pick-tagged prim faces this bug was found by, now flat
  across the re-tessellation instead of dropping to nothing.

Both were checked against the old behaviour before being trusted: with the reuse
lookup stubbed out (so the rebuild despawns and respawns as it used to), the
census fails at **frame 11 with no pick-tagged faces at all** — the hole this
task reported, reproduced on demand.
