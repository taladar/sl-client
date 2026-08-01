---
id: viewer-perf-prim-tessellation-cache
title: Cross-instance prim tessellation cache + shared mesh handles
topic: viewer
status: done
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

## Recovering copy-paste duplication (geometry only)

The real-world win is bigger than plain prims. SL has no instancing on the
wire, but builders copy-paste constantly — a stand of identical trees, a fence
of identical posts, a store vendor's wall of identical boxes — so a region
carries many *separate* objects that are geometrically identical apart from
position / rotation / scale. That duplication is **recoverable** from
transform-independent geometry keys, and it is worth doing **independently of
materials**:

- **Key the mesh cache on shape only — never fold the texture in.** Same-shape
  / different-texture prims are everywhere: the store vendor whose wall of
  identical boxes each shows a different product, one sculpt reused with many
  textures. A shape-only key still shares the geometry across all of them,
  whereas a (shape + texture) key would dedup almost nothing there.
- **Mesh / sculpt objects are the easy, high-value case:** copies reference the
  *same* mesh-asset / sculpt-map UUID, so grouping by (asset UUID, LOD) detects
  them directly — no tessellation involved, and trees / fences / furniture are
  usually mesh or sculpt. Likely worth doing before (or with) the prim
  fingerprint.
- **Prims** use the shape-param fingerprint above (per (fingerprint, LOD,
  face), so the per-face geometry is shared).

This is the primary, high-hit-rate win: it kills the tessellation storm
(texture-independent) and shares one vertex buffer + mesh prep across every
copy, even when their textures differ. It does **not** by itself collapse draw
calls when materials differ, and does **not** reduce entity count or
per-instance frustum culling (a separate lever — see
[[viewer-perf-pipeline-specialization-stalls]]). Collapsing the draws when the
texture matches too is the composing, separate job of
[[viewer-perf-material-intern]]. Excluded: anything whose geometry mutates per
frame (flexi) cannot share a mesh handle.

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

## Done (2026-07-31)

Implemented as the `GeometryCache` resource
(`sl-client-bevy-viewer/src/geometry_cache.rs`): shared `Mesh` handles
keyed `Prim(PrimShapeParams, PrimLod)` / `Sculpt(map, type, decoded
w×h)` / `Mesh(MeshKey, MeshLod)`, wired into the prim spawn + prim-LOD
paths (`spawn_cached_prim_faces`), the sculpt build, and the mesh-asset
submesh build. The cache holds only weak `AssetId`s; a spawn *revives* a
live shared asset via `Assets::get_strong_handle`, so asset lifetime
stays tied to live face entities and a periodic prune drops dead
entries — no teleport hook needed. `PrimShapeParams` gained `Hash`
(all-integer). The verification pass confirmed scale rides the
geometry-holder transform for prims/sculpts/meshes; the one leak found —
planar-texgen faces bake object scale into UV0 — is handled by sharing
those per quantized (mm) object scale instead of excluding them. Flexi
(per-frame mesh mutation), grass/trees (scale baked / own path), and
rigged meshes stay uncached. The F3 pipeline panel gained a
`geom entries/hit/partial/miss` line (and now starts below the top menu
bar, which used to cover its first lines).

Measured on the local OpenSim (region seeded with 5× copies of the nine
`rez_sample_prims` shapes, 3× mesh + sculpt OAR loads, 3 planar-texgen
boxes at two scales):

- A clean login: `entries 20, hit 127-128, partial 2, miss 31-32` —
  ~79% of geometry spawns reuse a live shared mesh with zero
  tessellation / conversion / GPU upload; under camera-motion LOD churn
  the hit count grew to 402 vs 64 misses (~86%).
- Worst single system invocation during the login rez-in spike
  (Tracy, pacing-independent; system self time): `update_objects`
  1503 µs → 694 µs, `apply_prim_lod` 882 µs → 419 µs (its command-apply
  1425 µs → 691 µs), `apply_object_meshes` 135 µs → 29 µs. (Whole-run
  self-time totals proved incomparable between runs — Wayland gates
  frame callbacks for occluded windows, and ~2 µs/frame idle system
  overhead × a 50× frame-count difference swamps the geometry work.)
- Visual regression: baseline vs cached screenshot pairs at an
  identical scripted camera are visually identical (prims of all seven
  volume types, sculpts, meshes, trees, grass, planar-texgen boxes at
  1 m and 2 m).
