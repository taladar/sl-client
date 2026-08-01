---
id: viewer-perf-per-object-face-merge-entity-count
title: Cut per-frame CPU by reducing world-object entity count (face merge / per-object cull unit)
topic: viewer
status: wont-do
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

## Won't-do — entity-count premise undercut; cost/risk too high (2026-08-01)

Investigated during the same aditi profiling push that shelved the octree cull
([[viewer-perf-world-frustum-culling-octree]]), reading the Bevy 0.19 extract /
visibility source. Dropped: the merge is a large, cross-cutting refactor whose
expected win is small and partly already captured, while the sub-60-fps costs
we actually care about lie elsewhere.

**Why it would barely help.**

- **Extraction is already incremental.** `extract_meshes_for_gpu_building`
  re-extracts only entities whose `ViewVisibility` / `GlobalTransform` / `Aabb`
  / `Mesh3d` actually changed, and `ViewVisibility`'s packed current/previous
  bits suppress change detection for an entity visible last frame *and* this
  frame. A settled, static-camera scene re-extracts almost nothing — the
  "~7 ms extract" is the O(N) `Changed`-filter *table scan* plus rez / motion
  churn, not per-face re-extraction of static geometry. Merge shrinks that scan
  by the face multiplier, but the scan is a few ms, not the frame.
- **The visibility sweep is parallel and off the critical path.**
  `check_visibility` is a `par_iter` (~1.4 ms wall across ~11 workers)
  overlapping the main thread — the finding that already shelved the octree.
  Fewer leaves shortens it marginally at best.
- **The frame is main-thread-bound + a pipelined render thread.** Render-thread
  submit *does* scale with drawn-mesh count, so merge would cut it — but it runs
  concurrently behind the main thread, so it only helps when it is the gating
  stage, and the shadow half of that render cost is cut far more cheaply by
  fewer cascades (caster × cascade) than by a per-object mesh merge.
- The sub-60 frames we care about look like specific per-frame churn / spikes
  (to be pinned with Tracy), not the steady-state per-face entity count.

**Why it would be hard.**

- **~15 modules assume one entity per Linden face.** Picking (`pick_object`,
  `hud_pick`, `avatar_pick`, `object_menu`, `edit_selection`) resolves the exact
  hit face entity; per-face material application (`materials.rs` PBR,
  `legacy_materials.rs`, `bump.rs`, `texture_anim.rs`) mutates an individual
  face entity's material *asynchronously* as assets / overrides arrive; per-face
  editing (`edit_material`, `edit_texture`), `render_priority` (per-face
  pixel-area LOD) and `media_prim` all key off the face entity. A merged
  `Mesh3d` needs a triangle-range → Linden-face map, and every one of those
  systems must become submesh-range-aware or exclude the faces it touches.
- **A single-face material change forces an object regroup.** A PBR override or
  a texture edit arriving for one face splits it out of its merged
  (object, material) group, so the object's merged meshes must be rebuilt —
  extra machinery on the async material paths.
- **It fights the just-landed content interning** (`1679dbda`): merged
  per-(object, material) meshes are unique per object, so the cross-object draw
  batching of partially-similar objects stops firing.

Net: a large, risky, instancing-defeating refactor for a speculative win that
change-gated extraction, parallel culling, and cascade tuning already blunt.
Revisit only if a Tracy capture pins a *specific* per-frame cost squarely on the
raw face-entity count.
