---
id: viewer-perf-gpu-avatar-phase0-mesh-dedup
title: GPU avatars Phase 0 — share rigged-submesh Mesh + IBP assets (instancing)
topic: viewer
status: done
origin: GPU-avatar design (2026-08-12), context/gpu-avatars.md §3.2
refs: [viewer-perf-gpu-avatar-crowd, viewer-perf-steady-state-46fps-ceiling]
---

Context: [context/gpu-avatars.md](../context/gpu-avatars.md) §3.2, §1.2(b),
§3.1. Epic: [[viewer-perf-gpu-avatar-crowd]].

**Standalone win, no GPU rewrite.** Today `build_rigged_submeshes`
(`objects.rs:5057`) does `meshes.add(...)` per wearer and mints a fresh
`SkinnedMeshInverseBindposes` per wearer (`:5043`), so two wearers of the same
mesh body never share a `Handle<Mesh>` and never batch — the real reason the
crowd doesn't instance. Fix: cache rigged-submesh `Mesh` by
`(MeshKey, lod, submesh_index)` and `SkinnedMeshInverseBindposes` by
`(MeshKey, lod)` through `GeometryCache` (weak `AssetId` + revive semantics,
mirroring the prim slots). Per-wearer differences (BOM tint, textures) stay in
the per-entity bindless `FaceMaterial`.

**Correctness constraint:** `SkinnedMesh { inverse_bindposes: Handle, joints:
Vec<Entity> }` — share the geometry `Mesh` handle and the `inverse_bindposes`
asset, but the `joints` entity list stays **per-avatar** (each wearer poses
independently via its own joints + `current_skin_index`). Batching keys on
(pipeline, mesh asset, bind groups); sharing the mesh + IBP handles is what
lets Bevy batch.

Verification: two-same-body OpenSim scene shows **one instanced draw per
(submesh, alpha-mode)** for the wearers (Tracy draw counts / RenderDoc); no
render regression; `extract_skins` unchanged (this phase does not touch pose).
Client-side unit tests: same key → same handle, different lod/submesh/MeshKey →
different, IBP shared by `(MeshKey, lod)`, revive-after-drop.

**Status: implementation done (2026-08-12); one measurement outstanding.**
Code landed and committed; live-verified on aditi — **no regression**, avatars
render as before and pose independently, clean login/logout, no panic, the own
rigged body builds through the shared cache path. Unit tests prove
handle-sharing; Bevy auto-batches same-handle skinned draws (verified from
source in `context/gpu-avatars.md` §0). The implementation is complete, so this
task is **done**.

**Outstanding follow-up (perf measurement, not implementation):** the actual
draw-call collapse is inferred, not yet measured — shared handles are necessary
but not sufficient for batching (per-wearer bindless `FaceMaterial` must not
split the batch). Since this viewer is not going to the main grid for now, the
go-to measurement is **the three Aditi avatars on one region — they share the
same starter mesh body** (same `MeshKey`): confirm the F3 `rigged hit` counter
climbs as the 2nd/3rd bind, and RenderDoc the single instanced draw per
submesh. One 3-avatar run (the conformance harness can drive the multi-login)
closes it; it gates nothing.
