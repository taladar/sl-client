---
id: viewer-mesh-stuck-low-lod-warm-cache
title: Shared / warm-cache meshes stuck at a coarse LOD, never refining on approach
topic: viewer
status: done
origin: noticed live on aditi after the async-fetch work (2026-08-11)
---

Context: [context/viewer.md](../context/viewer.md).

Symptom (live on aditi): some in-world meshes stayed at a coarse level of detail
and never sharpened as the camera approached — while others refined correctly.
The [[viewer-p21-2]] machinery is correct, but its live
verification only exercised the **cold-cache, single-instance** path. Any
**second-or-later instance of a shared mesh asset** (repeated furniture, fences,
plants, kitbashed linksets — very common in SL), or an object built after its
mesh had already decoded, took a warm-cache fast path that left it frozen at the
level the first instance decoded at (the coarse `INITIAL_MANAGED_LOD`).

Two independent gates, both fixed:

- **Gate 2 (`objects.rs`, the common one):** `build_object_geometry`'s warm mesh
  arm built the submeshes immediately and returned no `PendingGeometry::Mesh`,
  so the object's `mesh_rebuild` inputs — set only on the cold path in
  `apply_object_meshes` — were never populated. Its LOD-swap rebuild branch then
  never matched, so even when the shared geometry *did* upgrade the object kept
  showing its coarse submeshes. Fixed by threading a
  `mesh_rebuild: Option<PendingMesh>` out of `build_object_geometry` (the sixth
  tuple element, exactly like the existing `prim_rebuild` / `tree_rebuild`) and
  setting it on the `TrackedObject` at both build sites.
- **Gate 1 (`meshes.rs`):** `MeshManager::request` early-returned on
  `decoded.contains_key(id)` *before* registering the `managed` slot / retained
  handle, so a warm mesh whose `managed` slot had been dropped (e.g. first
  fetched boosted, or its earlier instance despawned) was never LOD-managed at
  all and `set_lod_for_area` no-op'd. Fixed with `ensure_managed`, which
  registers the managed slot + a retained (undriven, gate-free) `MeshRequest`
  for an already-decoded mesh — guarded to skip meshes that already have a
  handle or are rigged (worn attachments must stay finest, never be reduced).

Verified live on aditi: 385 `pixel-area LOD` swaps including `Low -> Medium`,
`Lowest -> Low`, `Medium -> High` as the camera moved; meshes visibly refine on
approach now.

Follow-up (separate): the *range* at which meshes reach full LOD is the
reference default. `for_distance` faithfully ports `LLVOVolume::calcLOD`, and
`DEFAULT_LOD_FACTOR = 1.0` equals the reference `RenderVolumeLODFactor` default.
The reference "Mesh Detail: Objects" slider scales exactly that factor (up to
~4×), which is why full LOD reaches much farther with a dialed-up reference
viewer. Exposing `lod_factor` as a preference is its own task — see
[[viewer-mesh-lod-factor-preference]].
