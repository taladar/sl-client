---
id: viewer-perf-per-object-face-merge-entity-count
title: Cut per-frame CPU by reducing world-object entity count (face merge / per-object cull unit)
topic: viewer
status: ideas
origin: wall-clock profiling that shelved the octree cull
  (viewer-perf-world-frustum-culling-octree, 2026-08-01)
refs:
  [viewer-perf-world-frustum-culling-octree, viewer-perf-probe-occlusion-skip]
---

Context: [context/viewer.md](../context/viewer.md).

The octree frustum-culling idea
([[viewer-perf-world-frustum-culling-octree]]) was shelved once wall-clock
profiling showed the cull itself is ~1.4 ms/frame and off the critical path.
That same aditi trace (no-culling steady state) showed where the frame **does**
go — a balanced ~27–30 ms main/render pipeline whose costs scale with the
number of **drawn entities**, not with the cull algorithm:

| cost (per frame) | ms | scales with |
| --- | --- | --- |
| render thread (draw + submit + present) | ~27 | drawn face-entity count |
| extract (`RenderExtractApp`) | ~7.4 | drawn entity count |
| PostUpdate visibility par-iters | ~1.4 wall | entity count × views |

Every prim is spawned as **one child entity per Linden face** (`objects.rs` —
each face carries its own `Aabb`, `Mesh3d`, `FaceMaterial`), so a region is tens
of thousands of face entities — a 2–6× multiplier on *every* per-entity,
per-frame pass: extraction, pipeline specialization, draw-command build,
GPU-buffer writes, transform propagation, and the visibility scan. Reducing the
entity count attacks **all of them at once**, which a faster culler does not.

## Directions

1. **Merge an object's same-material faces into one `Mesh3d`.** Bevy is one
   material per mesh, so faces that share a resolved `FaceMaterial` (already
   interned by content — see the material-intern work) can be combined into a
   single mesh entity per (object, material). A single-texture prim collapses
   from ~6 face entities to 1. Watch: per-face picking (`MeshRayCast` +
   `PrimFaceEntity`) and per-face material edits — the merged mesh needs a
   face-index → submesh-range map so a pick / override still resolves to a
   Linden face. Re-tessellation / LOD swaps must rebuild the merged mesh.
2. **A combined per-object `Aabb` as the cull/extract unit.** Even without full
   face merge, giving the object (or geometry holder) one bound and letting the
   faces ride it reduces the visibility-scan leaf count — but note the extract /
   draw cost is per *renderable mesh*, so this alone does not cut the big items;
   the mesh merge (1) is what removes draw entities.
3. **Compose with fewer views** ([[viewer-perf-probe-occlusion-skip]]): the
   per-entity passes run once per active view (main + each active reflection
   probe cube face + shadow cascades), so entity-count and view-count multiply.

Measure the same way (the [[viewer-perf-world-frustum-culling-octree]] won't-do
note plus `book/src/tools/profiling.md`): **steady-state frame time and the
main-thread schedule durations (extract / PostUpdate) plus the render-thread
Render schedule**, on the same aditi spot — never summed self-time.
