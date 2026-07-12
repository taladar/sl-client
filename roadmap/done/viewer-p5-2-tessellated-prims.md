---
id: viewer-p5-2
title: Tessellated prims
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 5 — Prim rendering in the viewer
---

Context: [context/viewer.md](../context/viewer.md).

**P5.2. Tessellated prims.** For a plain prim, tessellate with
`sl_prim` at a fixed High LOD and spawn one child entity per `PrimFace` (so
each face can carry its own material). Verify box / cylinder / sphere / torus
render correctly positioned on OpenSim. **Done:** `build_prim_faces`
tessellates a
`Prim`-category object (`PrimShapeFloat::from_params` → `tessellate(_,
PrimLod::High)`) and spawns one `Mesh3d` child per non-empty face
(`to_bevy_prim_mesh`), parented via `ChildOf` to the object entity so the
object's `Transform` carries the object scale / rotation / position and the
single SL→Bevy basis change; a shape-fingerprint change despawns and rebuilds
the face children (`despawn_prim_faces`), a motion-only update never
re-tessellates. Each face carries a `PrimFaceEntity { face_id }` marker for
the Phase 6 per-face texturing pass to key off. Until Phase 6 every face
renders with one shared neutral placeholder `StandardMaterial` (double-sided /
culling off, so a face shows regardless of winding). Two live findings: (a)
the object entity now also carries `Visibility::default()` — the `Mesh3d` face
children add `Visibility`, and Bevy's visibility propagation warns (B0004) if
the parent has none; (b) the hollow-cap MVP simplification from P3.4 is
visible on OpenSim — a hollow prim's cap fills its hole, so a hollow prim
reads as a solid-capped tube. Verified live on OpenSim (4 prims + 1 mesh + 1
avatar streamed and tessellated; prims render untextured — texturing is P6).
