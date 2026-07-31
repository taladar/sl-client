---
id: viewer-perf-material-intern
title: Intern face materials by content for shared handles + draw batching
topic: viewer
status: ideas
origin: profiling design discussion (2026-07-31)
blocked_by: [viewer-perf-prim-tessellation-cache]
refs: [viewer-perf-prim-tessellation-cache, viewer-profiling]
---

Context: [context/viewer.md](../context/viewer.md).

The second half of the copy-paste-duplication recovery, split out from the
geometry cache ([[viewer-perf-prim-tessellation-cache]]) because the two have
very different hit rates and compose independently.

Today every face builds its own material — `materials.add(...)` per face in
`objects.rs`, no content keying — so even faces that are textured identically
get distinct `Handle<FaceMaterial>`s. Bevy's automatic batching (GPU
preprocessing, already enabled) collapses draws only when **both** the
`Handle<Mesh>` *and* the material handle match, so distinct material handles
block batching even when the geometry cache has already shared the mesh.

## Proposed fix

Intern `FaceMaterial` by a fingerprint of its content — texture id + colour +
`uv_transform` (repeats/offset/rotation) + the PBR/legacy material params — so
identical faces reference one shared handle. Then, per face, an entity takes its
`Handle<Mesh>` from the geometry cache and its material handle from this intern;
when both match a sibling copy, Bevy batches them into one instanced draw for
free (no custom instanced-render path), with the transform staying per-entity.

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
  handle.
- Eviction/generation tracking mirrors the geometry cache (release on region
  teardown / teleport).
