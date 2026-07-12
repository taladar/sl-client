---
id: viewer-p12-3
title: Base-mesh .llm decode
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 12 — `sl-avatar`: skeleton & base body (pure crate)
---

Context: [context/viewer.md](../context/viewer.md).

**P12.3. Base-mesh `.llm` decode.** `basemesh.rs`: decode the legacy
Linden avatar mesh format → `BaseMesh { positions, normals, uvs, weights }`
(per-vertex skin weights to skeleton joints) + the mesh's morph-target deltas.
One decoder per base part (head, upper, lower, eyes, hair, skirt, eyelashes)
with their LOD chains. Distinct from `sl_mesh` (`LLMesh`). **Done:**
`BaseMesh::from_bytes` decodes a full base part (`lod="0"`) from `&[u8]` —
header transform + flags, per-vertex positions/normals/binormals/primary
(and optional detail) UVs, the per-vertex `VertexSkinWeight` (the single
on-disk weight float split into `{ joint, blend }` where `joint = floor(w)`
indexes the mesh's own skin-joint name table and `blend = w - joint` lerps to
`joint + 1`), triangle faces, the joint-name table, the `MorphTarget` deltas
(sparse per-vertex position/normal/binormal/UV deltas, read until the
`End Morphs` sentinel), and the `SharedVertex` remap table.
`LodMesh::from_bytes` decodes a reduced LOD (`lod="1"`..`"5"`): the same
binary shape but only the header transform + the reduced face list are
meaningful (faces index into the base part's vertices), so `vertex_count` is
one-past-the-largest referenced index. A forward-only `Cursor` reads
little-endian primitives via `f32::from_bits` / byte-fold shifts (the crate
lints forbid `from_le_bytes` and `as`). Follows Firestorm
`LLPolyMeshSharedData::loadMesh` / `LLPolyMorphData::loadBinary`. Committed
binary fixtures (`mini_basemesh.llm` 4 verts / 2 faces / 2 joints / 1 morph /
1 remap, `mini_basemesh_lod.llm`); `cargo test -p sl-avatar` (6 new tests).
