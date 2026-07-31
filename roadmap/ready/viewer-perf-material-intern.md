---
id: viewer-perf-material-intern
title: Intern face materials by content for shared handles + draw batching
topic: viewer
status: ready
origin: profiling design discussion (2026-07-31)
blocked_by: [viewer-perf-prim-tessellation-cache]
refs: [viewer-perf-prim-tessellation-cache, viewer-profiling]
---

Context: [context/viewer.md](../context/viewer.md).

The second half of the copy-paste-duplication recovery, split out from the
geometry cache ([[viewer-perf-prim-tessellation-cache]]) because the two have
very different hit rates and compose independently. The geometry half is
**done** (2026-07-31): identical prim / sculpt / mesh instances now share one
`Handle<Mesh>` per face through the `GeometryCache` resource
(`sl-client-bevy-viewer/src/geometry_cache.rs`), at a measured ~79% full-hit
rate on a login and ~86% under LOD churn. This item finishes the job for
whole-object duplicates: when the *materials* match too, Bevy's automatic
batching collapses the copies into instanced draws.

Today every face still builds its own material — `face_material(...)` /
`materials.add(...)` per face, no content keying — so even identically
textured faces get distinct `Handle<FaceMaterial>`s. Bevy batches draws only
when **both** the `Handle<Mesh>` *and* the material handle match, so distinct
material handles block batching even where the geometry cache already shared
the mesh.

## Approach (grounded in the implemented geometry cache)

- **Interception point:** `spawn_face_entity` (objects.rs) is now the single
  chokepoint every face path funnels through (prims, sculpts, meshes — cached
  and uncached), and it is exactly where `face_material` runs. Intern there,
  mirroring how the geometry paths intercept `meshes.add`.
- **Key:** a fingerprint of the material *inputs*: the decoded `TextureFace`
  content (texture id, tint bytes, repeats / offset / rotation → the
  `uv_transform`, glow, bump-shiny-fullbright byte, planar flag) plus the
  `TextureAlpha` mode the caller passes. The wire values are quantized, so
  re-encoding the face (or hashing the raw per-face wire bytes) gives an
  integer key with no float-hashing trouble — same trick as
  `PrimShapeParams`.
- **Lifetime:** copy the proven `GeometryCache` pattern verbatim — store weak
  `AssetId<FaceMaterial>`s, revive via `Assets::get_strong_handle`, prune
  dead entries on the same periodic sweep. No new eviction design needed.
- **Post-creation mutation is the real design work.** A face material is not
  immutable today:
  - `face_material` parks the material in `PrimTextures` until its texture
    decodes, then mutates it (texture handle, alpha upgrade). With a shared
    handle the same registration can arrive from several faces — the pending
    bookkeeping must tolerate duplicate registrations of one handle (the
    mutation itself is idempotent, same content → same result).
  - The later per-face material *rewriters* — PBR GLTF materials
    (`ObjectRenderMaterials` / materials.rs), legacy normal/specular
    (legacy_materials.rs), bump (bump.rs), per-face texture animation
    (`SlFaceExt` GPU params), media-on-a-prim — mutate a face's material
    in place. First iteration: **exclude** any face those systems touch
    (texture-animated, MoaP, PBR-/legacy-/bump-covered faces) from
    interning, exactly as the caveat below already planned; they keep
    per-instance materials. A later iteration can fold the PBR/legacy
    material id into the key instead.
- **Payoff check:** two same-shape same-texture copies (the rezzed duplicate
  rows on the local grid) should collapse to ~one instanced draw; verify via
  the F3 draws gauge / Tracy render spans, and confirm the store-vendor case
  (same shape, different texture) still shares the mesh while keeping
  separate draws.

## How it composes with the geometry cache

- **Shape matches, texture differs** (store vendor's wall of identical boxes,
  each a different product): geometry cache shares the mesh (one tessellation,
  one buffer); this intern does *not* fire on the differing face, so those draws
  stay separate — the geometry win is still fully realised. This is the common,
  high-value case for geometry-only dedup and why the two are split.
- **Shape and texture both match** (fence posts, a tree stand, a tiled floor):
  shared mesh *and* material → Bevy collapses them to ~one instanced draw.

So this item is the *bonus* that turns matched-texture duplication into a
draw-call win on top of the geometry cache's tessellation/GPU-memory win.

## Caveats

- `uv_transform` is material state, so faces differing only in repeats/offset
  get distinct materials and will not batch unless that placement is also
  identical (or later moved into per-instance data).
- Neither this nor the geometry cache reduces entity count or per-instance
  frustum culling — that is a separate lever (see
  [[viewer-perf-pipeline-specialization-stalls]]).
- Exclude materials that mutate per frame: per-face texture animation,
  media-on-a-prim, and any GPU-time-driven `SlFaceExt` effect cannot share a
  handle (see the mutation analysis above).
- Eviction/generation tracking mirrors the geometry cache (release on region
  teardown / teleport falls out of the weak-id + prune pattern).
