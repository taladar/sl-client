---
id: viewer-p7-1
title: Mesh geometry
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 7 — Mesh objects
---

Context: [context/viewer.md](../context/viewer.md).

**P7.1. Mesh geometry.** For `SculptOrMeshKey::Mesh(_)`, fetch and
decode the mesh **through the shared `sl_mesh::MeshStore`** — counterpart of
the `TextureStore` the Phase 6 texturing drives (weak-ref dedupe,
off-thread decode, Firestorm per-UUID `.mesh` disk cache, LOD-aware). Mirror
the P6 `TextureManager` shape: a viewer `MeshManager` resource holding a
`MeshStore` over a `BevyMeshFetcher` (cap URL from `SlCapabilities`;
`GetMesh2` / `GetMesh`), fetch each mesh on a background `IoTaskPool` task,
poll it, and announce it with a `MeshDecoded` message the object system
reacts to. Do **not** decode on the render thread or drive the raw
`Command::FetchMesh` / `MeshReceived` path — that is the low-level
equivalent the Phase 6 texture work deliberately moved off of. Convert each
decoded submesh with `to_bevy_mesh`, spawn one child entity per submesh, and
texture it via the Phase 6 `face_material` / `TextureManager` path. Verify
against the provisioned OpenSim mesh prim (`slclient-mesh.oar`). **Done — via
the shared `MeshStore`, mirroring the P6 texture pipeline exactly.** A new
`meshes.rs` module owns a `MeshManager` resource (a `MeshStore` over a
`BevyMeshFetcher` whose `GetMesh2` / `GetMesh` cap URL is refreshed from
`SlCapabilities`); each mesh is fetched on a background `IoTaskPool` task
(blocking HTTP off-thread, decode on the store's own `rayon` pool at
`MeshLod::FINEST`), and `poll_meshes` folds a finished decode into a shared
cache and announces it with a `MeshDecoded` message. In `objects.rs` a mesh
object requests its asset through the manager and, once the geometry is
available (immediately if already cached, else when `apply_object_meshes`
reacts to `MeshDecoded`), spawns one child entity per non-empty submesh via
`to_bevy_mesh`, textured through the same Phase 6 `face_material` path — each
submesh mapping to its Linden `TextureEntry` face slot (empty `NoGeometry`
submeshes are skipped but still count as a face index). A mesh object waiting
on its asset holds a `PendingMesh` (mesh key + the object's texture-entry
bytes); the shared prim/mesh geometry build is routed through one
`build_object_geometry` so a shape/category change rebuilds correctly. The
mesh geometry stays in the object's local Second Life space; the object
entity's `Transform` carries the object's scale / rotation / position and the
single SL → Bevy basis change (mesh positions are dequantized to their
normalized position domain, not pre-multiplied by scale — matching the core
viewer unpack). New re-export: `CAP_GET_MESH` / `CAP_GET_MESH2` from
`sl-client-bevy` (the mesh mirror of P6's `CAP_GET_TEXTURE`). Verified live
on OpenSim: the provisioned mesh prim is classified, fetched over `GetMesh`,
decoded off-thread, and its submesh entity spawned and textured; the on-disk
cache populates under `~/.cache/sl-client-bevy-viewer/meshcache`. **Live
finding + fix (shared with prims/terrain):** the shared `face_material` was
switched from the P5.2 double-sided / culling-off placeholder to
**single-sided (default back-face culling)** — Second Life renders a face
only from its front, so a one-sided surface (a flat mesh quad, a prim cut
face) must be invisible from behind rather than doubled. This is safe because
the SL → Bevy basis change is a proper rotation (determinant `+1`, handedness
preserved), so the outward windings that `sl_prim` tessellation and
`sl_mesh` decode already produce stay front-facing under Bevy's CCW culling.
Verified
live: the provisioned flat mesh quad is now visible only from its front
(top), and regular prims still render solid with no missing / inside-out
faces.
