---
id: viewer-perf-world-frustum-culling-octree
title: Spatial (octree/BVH) frustum culling for world meshes
topic: viewer
status: wont-do
origin: Tracy profiling of Aditi rezzing (2026-08-01) — frustum culling is the
  dominant sustained per-frame cost
refs:
  [viewer-perf-ui-layout-gate-open-widget-churn, viewer-perf-probe-occlusion-skip]
---

Context: [context/viewer.md](../context/viewer.md).

The 2026-08-01 full-capture analysis of Aditi rezzing found **visibility /
frustum culling is the single dominant sustained cost of the average frame** —
~**16.9 ms/frame** of (parallel) CPU work, well above the material / UI / probe
categories the recent perf work targeted. Breakdown (self-time, across worker
threads):

| pass (`bevy_camera::visibility` unless noted) | ms/frame |
| --- | --- |
| view-visibility propagation par_for_each | 6.3 |
| `check_visibility` frustum test par_for_each | 2.2 |
| Aabb cull par_for_each | 1.8 |
| `old_entity_cpu_culling` | 0.8 |
| `check_dir_light_mesh_visibility` (shadow casters) | 2.8 |

Two compounding causes:

1. **Per-face cull granularity.** Each `PrimFace` is its own `Aabb`-managed
   entity (`objects.rs` — one child entity per face so each can carry its own
   `FaceMaterial`), and the object root has no `Mesh3d`/`Aabb`, so the **face**
   is the cull unit. Bevy frustum-tests every face entity — ~2–6× the object
   count. A full region is tens of thousands of face entities.
2. **Flat, linear, per-view culling.** Bevy's `check_visibility` is an O(N)
   sweep over all renderable entities with **no spatial acceleration** (no
   octree/BVH), and it runs **once per view**: the main camera, *each shadow
   cascade* of the directional light (the separate
   `check_dir_light_mesh_visibility`), and every reflection / environment probe.
   So the cost is `entities × views`, done as a linear scan.

The reference viewer (LL / Firestorm) does neither: it partitions the region
into a **spatial octree** (`llspatialpartition`), culls *octree nodes* against
the frustum (an out-of-view cell rejects everything inside it in one
bounding-box test) and only descends into visible cells — roughly O(visible
cells) instead of O(all objects). Faces are grouped into render batches *within*
a visible spatial group and are never individually frustum-tested. It wins on
both counts: coarser cull unit **and** hierarchical rejection.

## Directions

1. **Quick win — cull per object, not per face.** Give each object root a
   combined `Aabb` spanning its faces and make it the cull unit; add
   `NoFrustumCulling` to the face entities. `NoFrustumCulling` only skips the
   frustum *test* — the face keeps its `Aabb`, so `MeshRayCast` picking still
   works (the constraint `objects.rs:2160` documents). Drops N from face-count
   to object-count without a new data structure. Watch the flexi bent-geometry
   case (the per-frame `Assets::get_mut` refreshes the face `Aabb`; the object
   root's combined bound must be refreshed too, or a bent flexi could cull
   wrongly).
2. **Real fix — a spatial hierarchy feeding visibility.** An octree/BVH over the
   world meshes (grouped by linkset / region cell), with a `check_visibility`
   replacement that culls nodes hierarchically and marks contained entities
   visible in bulk — reused across all views (main, shadow cascades, probes).
   Bevy has nothing built-in for this, so it is a substantial custom system;
   it is the piece that would actually match the reference viewer and remove the
   culling floor for a full region. Composes with
   [[viewer-perf-probe-occlusion-skip]] (fewer views to cull for) and the
   per-face → per-object change above (fewer leaves in the tree).

Measure with the same rez capture (`tracy-capture` self-time over a rez
window; the `visibility` / `check_visibility` /
`check_dir_light_mesh_visibility` zones) before/after.

## Won't-do — the premise was a measurement artefact (2026-08-01)

Investigated and dropped after wall-clock profiling on aditi (full writeup in
`book/src/tools/profiling.md`; method note in the sl-client skill). **The "17 ms
frustum culling" was summed self-time across ~11 worker threads, not frame
time.** `tracy-csvexport`'s aggregate sums each zone across every thread, and
`check_visibility` is a `par_for_each` that parallelises ~10× — its real
wall-clock is **~1.4 ms**, on a worker thread partly hidden behind the main
thread, **off the critical path**.

Controlled A/B (same aditi spot, octree pre-cull off): Bevy's **entire** stock
frustum culling is worth only **~2.5 ms/frame** (no-cull 31.7 ms → cull 29.3 ms,
steady state). An octree only makes the *decision* cheaper; it produces the same
visible set Bevy already computes, so its ceiling is **< 1 ms** of a ~29 ms
frame. Not worth a substantial custom `check_visibility` replacement.

Both directions were prototyped and reverted:

- **Direction 1** (per-face → per-object via `NoFrustumCulling`) is unsound: the
  shadow scan filters `With<Mesh3d>` and treats `NoFrustumCulling` faces as
  *unconditional* casters in every cascade — it would regress shadows.
- **Direction 2** (octree feeding a global-`Visibility` pre-cull) can only hide
  linksets invisible to *every* view; the sun cascades keep most objects
  globally visible, so it culls almost nothing. A full per-view
  `check_visibility` replacement could cut the decision cost, but that decision
  is only ~1.4 ms.

The frame is a balanced ~27–30 ms main/render pipeline gated by **drawn-object
count** (render-thread submit ~27 ms, extract ~7.4 ms) and PostUpdate's
*non-visibility* work (~10 ms: transform propagation + UI layout). The real
lever is fewer entities — see
[[viewer-perf-per-object-face-merge-entity-count]] — and fewer views
([[viewer-perf-probe-occlusion-skip]]), not a faster cull.
