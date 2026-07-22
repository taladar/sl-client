---
id: viewer-perf-prim-tessellation-cache
title: Cross-instance prim tessellation cache + shared mesh handles
topic: viewer
status: ideas
origin: performance survey of the implemented viewer (2026-07-22)
refs: [viewer-profiling]
---

Context: [context/viewer.md](../context/viewer.md).

Every plain prim tessellates its own geometry independently:
`build_prim_faces` (`objects.rs:1415-1427`) on spawn and
`apply_prim_lod` (`objects.rs:2594-2642`, `tessellate(&shape, desired)`
at `2622`) on every LOD transition. There is **no cache keyed by (shape
parameters, LOD)** — verified: the only related caches are
`PrimLodTargets`/`TreeLodTargets` (LOD *targets*, not geometry), and
`render_priority.rs:230-232` explicitly notes "Each prim tessellates its
own shape … there is no cross-instance aggregation."

SL regions are full of geometrically identical prims: default boxes and
cylinders, copied builds, linkset repeats. Each currently re-derives the
same vertex data from scratch — again on every LOD change as the camera
moves, which makes camera travel through a dense build a repeating
tessellation storm rather than a one-time cost.

## Proposed fix

An `Arc`-shared tessellation cache keyed by a fingerprint of the shape
parameters plus `PrimLod`:

- `tessellate` result cached as `Arc<PrimTessellation>`; cache hit skips
  all geometry work on spawn and LOD change.
- One step further: share the resulting **`Mesh` asset handles** per
  (fingerprint, LOD, face) so identical prims reference the same GPU
  buffers — reducing GPU memory and mesh-preparation work, not just CPU.
- Precondition for sharing: per-instance **scale must live in the
  `Transform`**, not be baked into the mesh, and per-face texture
  placement must stay in material/UV-transform space (it does — face
  `uv_transform` is material state). Any prim parameter that changes
  vertex output (path/profile/cut/twist/hollow/taper/shear…) belongs in
  the fingerprint; verify nothing else instance-specific leaks into the
  mesh before enabling handle sharing.
- Eviction: LRU or generation-based, since a region teardown / teleport
  should release the cache.

## Estimated impact

Medium: this is not steady-state per-frame CPU but dominates the two
worst spike scenarios — initial scene load and LOD thrash while the
camera moves. Dedup factor in real builds is large (often 10:1 or more
identical-shape prims); a cache turns those from N tessellations into
one, plus N transform inserts, and handle sharing multiplies GPU-memory
savings
by the same factor. Measure scene-login wall time + `apply_prim_lod`
zone totals during a camera fly-through ([[viewer-profiling]]).

Confidence: medium-high — absence of the cache verified; the
scale-in-transform precondition needs the verification pass described
above before handle sharing is switched on (the pure tessellation cache
is safe regardless).
