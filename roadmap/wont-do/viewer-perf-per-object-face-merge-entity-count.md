---
id: viewer-perf-per-object-face-merge-entity-count
title: Cut per-frame CPU by reducing world-object entity count (face merge / per-object cull unit)
topic: viewer
status: wont-do
origin: wall-clock profiling that shelved the octree cull
  (viewer-perf-world-frustum-culling-octree, 2026-08-01); revived 2026-08-16
refs:
  - viewer-perf-world-frustum-culling-octree
  - viewer-perf-probe-occlusion-skip
  - viewer-perf-render-app-bound-frame
  - viewer-perf-pipeline-specialization-stalls
---

Context: [context/viewer.md](../context/viewer.md).

## Re-shelved 2026-08-16 — Step 0 measured, the gate was not met

The revival below (kept in full) gated the refactor on **Step 0: measure the
per-entity serial ceiling first, proceed only if it justifies the risk.** That
measurement is now done and the gate **fails** — so the task is re-closed as
won't-do with fresh numbers, not shelved on the old assumptions.

**Measurement:** a 61.6 s `profile-tracy` release capture on aditi (primary
avatar), steady-state window `t > 25 s` where the scene had plateaued, n=703
frames. Method per `book/src/tools/profiling.md` (per-instance wall-clock via
`-u`, never summed self-time). The new `entity_diagnostics` plots gave the exact
population: **16 877 faces / 4 014 objects** (≈4.2 faces/object, so the
best-imaginable merge ratio is ~4×), 17 251 `Mesh3d`, 5 avatars.

**Critical path (steady, median per frame):**

| leg | ms | note |
| --- | --- | --- |
| `schedule{Render}` (thread 2) | **46.8** | the gating leg — CPU-bound, GPU ~4 ms idle |
| `schedule{Main}` (thread 1) | 25.2 | runs concurrently, **not** gating |
| Frame plot | p50 36.3 / p90 45.3 / p99 74.6 | |

So render **is** the gating CPU leg — the one thing the revival got right and
the old won't-do had wrong. But the merge only attacks the per-entity *tail* of
it, and the tail is small:

**Render-leg composition (per frame, steady):**

| bucket | ms | merge effect |
| --- | --- | --- |
| `submit_pending_command_buffers` | 7.76 | immovable (per view×pass; ~18 views) |
| `main_transparent_pass_3d` | 6.09 | immovable (per user item, unbatchable) |
| `main_opaque_pass_3d` | 2.59 | partly reducible **but instancing-risk** |
| gpu_preprocess (unpack/build/clear/early) | ~1.7 | per-batch — merge can **raise** it |
| queue+collect+specialize+extract+prepare (mesh) | ~4.0 | **merge-targetable, serial** |
| queue_shadows + specialize_shadows | ~2.3 | merge-targetable (cheaper via cascades) |

**The ceiling:** merge-targetable serial per-mesh work is
**~6.3 ms of the 46.8 ms gating leg (~13 %)**. A *perfect* 4× reduction saves
~4–5 ms → render leg ~42 ms, frame p50 ~33 ms (**~8 % best case**). That best
case is unreachable — the `internable()` exclusion set (PBR / legacy / bump /
media / texture-anim faces) cannot merge, and those dominate exactly on the
material-rich aditi regions where P27.2/27.3/27.4 found dozens–hundreds of them,
so the mergeable fraction (and the win) is smallest where the frame is worst.
Meanwhile merging per-(object, material) makes each mesh unique per object,
defeating the cross-object mesh+material batching interning enables, so
opaque-pass / gpu_preprocess / submit can **regress**. The Main leg is not
gating, so cutting its per-entity `PostUpdate` work buys nothing until it would
exceed Render.

**Where the frame time actually is:** the two biggest render-CPU costs — submit
(7.76 ms, per view×pass) and the transparent pass (6.09 ms, per user item) — are
both entity-count-**immovable**. The data points *away* from face-merge and
*toward* **fewer views / fewer shadow cascades**
([[viewer-perf-probe-occlusion-skip]] and cascade tuning) as the real render-CPU
levers — the same conclusion the 2026-08-01 won't-do reached, now confirmed with
render as the gating leg.

**Verdict:** a ≤~8 % best-case win that shrinks on the regions that matter, with
a real instancing regression risk, does not justify the ~15-module
cross-cutting, submesh-range-map refactor the "Why it would be hard" section
below still accurately describes. Re-closed. Re-open only if the immovable head
(submit + transparent + view count) is cut first and the per-entity tail becomes
the new gating cost.

## Revived 2026-08-16 — the won't-do escape clause tripped

The 2026-08-01 won't-do (kept in full below) closed with: *"Revisit only if a
Tracy capture pins a specific per-frame cost squarely on the raw face-entity
count."* Post-avian / post-pick-warm captures partly do, and change the picture
the original decision rested on — but **not entirely**, so this is revived with
eyes open, and gated on a measurement before the big refactor.

**What changed since 2026-08-01:**

- **The render thread is now the gating leg, and it is CPU-bound.** Main is down
  to ~25.7 ms (avian removed, pick-warm change-driven) while the render app is
  ~50 ms with **GPU only ~4.3 ms/frame** — the RX 7900 XTX is ~92 % idle. The
  2026-08-01 note assumed render "runs concurrently behind the main thread, so
  merge only helps when it is the gating stage." It **is** the gating stage now.
- **Per-entity serial work sits on the render critical path.**
  `queue_material_ meshes` (~1 ms), `collect_meshes_for_gpu_building` (~0.8 ms),
  the transparent phase-item build, and per-entity bind-group prep are serial
  render-thread work that scales with drawn-entity count; the main-thread
  `check_visibility_cpu_ culling` (~1.2 ms serial) + `calculate_bounds` are
  per-entity too. Collapsing a prim's ~faces into one entity is ~4× fewer
  entities through all of them.

**What the won't-do got right and still holds (do not re-learn the hard way):**

- **The two biggest render-thread zones are NOT entity-count.**
  `submit_pending_command_buffers` (~6 ms) is per **view × pass** (one command
  buffer per view), and `main_transparent_pass_3d` (~4 ms CPU) is per
  transparent *item*. Merge does not reduce the command-buffer count, and
  transparent-item count is user-generated SL content we cannot reduce; Bevy
  also can't batch transparent (depth-sorted, only adjacent-after-sort items
  merge). Fewer views is off the table too — probe cadence must stay
  SL-faithful, and running probes less often caused **huge spikes** because Bevy
  discards pipeline/mesh caches that go unused (the probe-cadence lesson). So
  merge attacks the per-entity *tail* of the render leg, not its head.
- **Much per-entity work is parallel / change-gated.** `gpu_preprocess` and
  `check_visibility` are `par_iter` across ~11 workers (small wall-clock);
  extraction is `Changed`-gated. Merge shrinks their *summed* work but only the
  *serial* portion is on the critical path.
- **Merge defeats cross-object instancing.** Per-(object, material) merged
  meshes are unique per object, so the content-interned cross-object draw
  batching (`viewer-perf-material-intern`) stops firing for them. GPU is idle so
  more draws is fine on the GPU, but more *distinct* draws can mean more CPU
  encode / submit — the very costs we are trying to cut. This must be measured,
  not assumed away.

**So the honest expectation:** entity-count reduction is the best *remaining*
lever (transparent-item count and view count are both immovable), but its
ceiling is the per-entity **serial** render + main work (order ~5–10 ms, much of
it on the non-gating Main leg), **not** the ~10 ms of submit + transparent that
dominate the render head. It is unlikely to move the ~53 ms median dramatically
on its own; treat it as removing a scaling factor (so the frame degrades more
gracefully as regions get denser / more views activate) rather than a median
win.

**Step 0 (gate the refactor): measure the ceiling first.** Before the
cross-cutting rewrite, bound the actual serial, on-critical-path per-entity
cost: from a Tracy `-u` unwrap, sum the render-thread serial systems that scale
with entity count (`queue_material_meshes`, `collect_meshes_for_gpu_building`,
per-entity phase-item build, bind-group prep) at the current ~14 k faces, and
estimate the ~4× reduction — and separately confirm merge does not inflate
submit/encode via lost instancing (a quick A/B on a merged vs unmerged subset).
Proceed with the full refactor only if that ceiling justifies the risk below.

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

## Historical won't-do (2026-08-01) — superseded by the revival above

Kept verbatim as the record of why this was shelved and which hazards remain
real. The "why it would barely help" bullets are the ones the 2026-08-16 data
partially overturned (render is now the gating CPU-bound leg); the "why it would
be hard" bullets are all still true and are the risk this task must manage.

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
