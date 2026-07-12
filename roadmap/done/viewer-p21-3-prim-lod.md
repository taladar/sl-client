---
id: viewer-p21-3
title: Prim LOD
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 21 — Distance / pixel-area LOD
---

Context: [context/viewer.md](../context/viewer.md).

**P21.3. Prim LOD.** Replace the fixed `PrimLod::High` with
a distance / area-selected `sl-prim` LOD tier (`LLVolumeLODGroup`);
re-tessellate on change. **Done:** a new `PrimLod::for_distance`
(`sl-prim`) selects the tessellation tier from radius / distance ×
`RenderVolumeLODFactor`. The LOD-tier selection is the *same*
`LLVolumeLODGroup` computation the reference viewer runs for a prim and a
mesh (`LLVOVolume::calcLOD` picks a volume's detail before it matters
whether the geometry is client-tessellated or asset-backed), so rather than
duplicate the trig it delegates to the P21.2 `sl_proto::MeshLod::for_distance`
and maps the resulting tier onto the matching `PrimLod` by index (both enums
are coarsest-first with identical `0..=3` indices). A plain prim is now
tessellated at a coarse `INITIAL_MANAGED_PRIM_LOD` placeholder and the
render-priority driver refines it: `drive_render_priority` computes each
prim's `PrimLod` from its full scale-vector length + camera distance (the same
`getScale().length()` radius the mesh LOD pass uses, **not** the half-diagonal
pixel-area radius) and records it in a new `PrimLodTargets` resource, which a
new `apply_prim_lod` system drains to re-tessellate any prim whose desired
level differs from its current one — the CPU-tessellation mirror of
`apply_object_meshes`' fetch-driven mesh LOD swap, but with no async fetch
(prim geometry is built on the spot). Each `TrackedObject` retains a
`PendingPrim` (shape + texture entry + scale + priority) so a swap can rebuild
without the live `Object`; only a plain prim carries it (a sculpt tessellates
from its decoded map with no `PrimLod` input, a mesh from fetched blocks), so
neither is prim-LOD managed. Since each prim tessellates its own shape there
is no cross-instance aggregation (unlike a mesh asset shared by many objects).
The crosshair pick tool (`P`) gained a prim-LOD readout alongside the P21.2
mesh one. Verified live on OpenSim: the Default Region's prims each start at
the `Low` placeholder and the driver upgrades them within a frame to
`Medium` / `High` by on-screen size (a stack of tori / cylinders resolved to a
mix of Medium and High, larger / nearer prims finer), no errors, avatar +
terrain unaffected.
