---
id: viewer-local-mesh
title: Local mesh — live-reload mesh from disk
topic: viewer
status: ideas
origin: Vintage-parity coverage audit (2026-07-22)
refs: [viewer-local-textures, viewer-mesh-gltf-import]
---

Context: [context/viewer.md](../context/viewer.md).

Firestorm's "local mesh": load a mesh file from disk and apply it to an
in-world object **locally only**, with live reload on file change — the
mesh sibling of [[viewer-local-textures]], letting creators iterate fit
and LODs in-world before paying for upload. Import path shares the glTF
ingestion of [[viewer-mesh-gltf-import]]; the applied object swaps its
sculpt/mesh geometry client-side.

Idea-stage: worth doing after local textures prove the stand-in-asset
plumbing; check how FS binds LODs and rigging in the local case (worn
rigged local mesh is their headline use).

Reference (Firestorm, read-only): `vjlocalmesh` /
`floater_vj_local_mesh.xml`.
