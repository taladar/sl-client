---
id: viewer-p3-1
title: Types & LOD
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 3 — `sl-prim` (pure Linden prim tessellation)
---

Context: [context/viewer.md](../context/viewer.md).

**P3.1. Types & LOD.** `PrimLod` newtype + a detail→step-count map
(details `{1.0, 1.5, 2.5, 4.0}`, profile sides `6 * detail`); output
`PrimMesh { faces: Vec<PrimFace> }`, `PrimFace { positions, normals, uvs,
indices, face_id }` (mirror `sl_mesh::DecodedMesh` / `Submesh`). Confirm or
derive the float `PrimShape` input from `PrimShapeParams`.
