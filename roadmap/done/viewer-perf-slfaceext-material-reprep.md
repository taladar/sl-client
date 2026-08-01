---
id: viewer-perf-slfaceext-material-reprep
title: Investigate per-frame re-prep of SlFaceExt face materials during rez
topic: viewer
status: done
origin: Tracy profiling of Aditi rezzing (2026-07-30)
refs: [viewer-perf-ui-layout-per-frame-relayout,
  viewer-perf-pipeline-specialization-stalls]
---

Context: [context/viewer.md](../context/viewer.md).

## Finding (2026-07-31): no per-frame mutation bug; the cost is rez batching

Bucketing the `prepare_erased_assets<…SlFaceExt>` self-time (the actual re-prep
cost) over the 115.6 s / 3568-frame `tracy-grab-after` capture answers the
roadmap's own criterion directly:

| Phase | prepare mean/frame | max |
| --- | --- | --- |
| Rezzing (first ~58 s) | 0.3 – 1.35 ms (spiky) | up to 33 ms |
| Stationary, rezzed (last ~58 s) | 0.010 – 0.021 ms | ~0.1 – 0.37 ms |

The re-prep decays to the empty-system floor at standstill, so it **tracks the
number of changed materials** (streaming), not a fixed per-frame set. Code
review confirms every `Assets<FaceMaterial>` mutator is change-gated (a
non-mutating `get` compare before `get_mut`): `drive_texture_animations`,
`apply_bom_face_materials`, `apply_hud_fullbright` (`Changed` filter + value
guard), `apply_bump_normals` (drains `pending` on decode), and the avatar
bake/coarse/object systems (dirty-set / message gated). So the original 3444
`Changed` hits (≈17/frame) were genuine rez churn, not a mutation storm.

The residual per-frame standstill cost is **not** SlFaceExt re-prep:
`check_entities_needing_specialization<…SlFaceExt>` is Bevy's fixed
~0.15 ms/frame per-entity tick check, and `specialize_material_meshes`
(~4.5 ms/frame, does not decay) is the generic Bevy per-(view × entity) system —
that belongs to the `viewer-perf-pipeline-specialization-stalls` task.

## The spikes are spawn-driven, not texture-driven (same-scene A/B)

The re-prep distribution is 3432 frames ≤1 ms but **21 frames >5 ms** (worst
33.5 / 27.4 / 19.8 ms). A same-scene A/B on Aditi (baseline `tracy-grab-after`
vs a capture with the texture-apply budget on) settled where the spikes come
from:

- The texture-apply budget alone did **not** move the spikes (33.5→29.9 ms max,
  >5 ms frames 21→24 — noise). Its deferred-drain barely engaged
  (`drain_deferred_face_textures` peaked at 0.31 ms), because our repeated-login
  tests serve textures from local cache, so that path is cheap here.
- Correlating the spike frames (39.5 s, 61.4 s): `apply_prim_textures` / drain
  were ~0, but the object-spawn systems (`update_objects`,
  `apply_object_meshes`) were elevated. The 29 ms is the render world building
  bind groups for the batch of **new** face materials created when a linkset
  streams in and `update_objects` spawns all its faces in one frame
  (`materials.add()` per face). Physics agrees: a 29 ms frame is hundreds of
  materials, far above the 64-material texture budget, so it cannot be the
  (capped) texture path.

Causal model (matches the cache observation): the spike lands on whichever input
finishes the face's dependency chain **last**. Cache-warm (our tests), textures
resolve early and geometry arrives last → the spawn batch spikes. Cache-cold
(live), geometry spawns flat first and textures trickle in → the texture/image
path spikes. So all three orderings are budgeted.

## Fix: three per-frame budgets (progressive rez)

1. **Material re-preps** — `TextureApplyBudget.reprep_*` (default 48,
   `SL_VIEWER_FACE_REPREP_BUDGET`): caps face-material `get_mut`s in the
   texture-apply systems; overflow → `DeferredFaceTextures`, drained FIFO by
   `drain_deferred_face_textures`. Kept for the cache-cold live case even though
   it was quiet in the cache-warm A/B.
2. **Image builds** — `TextureApplyBudget.image_*` (default 6,
   `SL_VIEWER_TEXTURE_IMAGE_BUDGET`): caps `build_prim_image` RGBA uploads/frame
   (each ~1.5 ms; a separate ~40–55 ms `apply_prim_textures` spike from a
   cache-warm decode burst); over-budget textures stay parked and
   `patch_parked_decoded_textures` builds them over later frames.
3. **Object spawns** — `SpawnBudget` (default 16 geometry-builds/frame,
   `SL_VIEWER_OBJECT_SPAWN_BUDGET`): `update_objects` now buffers the object
   stream into a FIFO `PendingObjectEvents` queue and drains it at ≤N
   geometry-builds/frame (`apply_object` returns whether it built geometry — a
   new spawn or a reshape/retexture re-tessellation; a move / remove is free).
   Strict FIFO preserves arrival order (root-before-child,
   update/remove-after-add), so this is pure progressive pop-in.

Unit tested in `textures.rs` (reprep + image gate defer/drain, despawned-face
drop) and `objects.rs` (`drain_budgeted` FIFO / builds-only budgeting).

## Verified (2026-07-31): modest tail-latency win, not an FPS cure

Three-way same-scene Aditi A/B (baseline → 48/24/64 → **16/6/48**), true
frame-time from "finished frame" markers over the rez window:

| | median | p95 | p99 |
| --- | --- | --- | --- |
| baseline | 32.5 ms | 55.7 ms | 83.7 ms |
| 16/6/48 | 32.7 ms | 57.2 ms | **68.7 ms** |

`prepare<…SlFaceExt>` max fell monotonically 33.5 → 19.3 → **13.0 ms** (>20 ms
frames 2 → 0). Flattening an *occasional* spike (~20 of 3500 frames) can only
move the **tail**, and it did (p99 83.7 → 68.7 ms); the **median ~32 ms /
~30 fps rez floor is untouched** — sustained per-frame load
(`specialize_material_meshes` ~4.5 ms + general render), i.e.
`viewer-perf-pipeline-specialization-stalls`, not this task. Shipped `16/6/48`
as the defaults (all env-tunable).

Tooling: the large post-fix trace crashed `tracy-csvexport` (SIGSEGV in
ppqsort's parallel branchless partition). Fixed in the tracy fork
(`taladar/bevy`, commit `c3e0f6d7`) by reverting to sequential ppqsort.

Follow-up (a) **done**: the LOD re-decode path in `apply_prim_textures` now
shares the image-build budget — a re-upload behind an existing handle
(`refresh_lod_image`, ~1.5 ms each) is gated by `defer_lod_reupload`, and the
overflow queues on `PrimTextures::pending_lod` for `drain_lod_reuploads` (the
last, lowest-priority step of the texture-apply chain, so a face's first
appearance always wins the frame's budget over a mere refinement). Gating on the
image budget also bounds the LOD reprep (fewer textures processed → fewer
materials re-marked). Unit tested (`lod_reupload_gate_defers_and_dedups…`). A
live confirmation of the ~40 ms `apply_prim_textures` spike flattening still
wants a camera-movement capture (LOD upgrades trigger on approach, which the
log-in-and-rez pattern does not reliably exercise).

Follow-up (b) **done**: `update_objects` no longer clones every incoming object
event. It now drains any earlier-frame backlog first, then applies new events
**inline (no clone)** while the queue is empty and the budget holds, cloning
only the overflow it must defer (`apply_pending_object_event` handles the owned
backlog events; the inline path calls `apply_object` / `remove_object` on the
borrowed event). Strict FIFO is preserved — the moment one event is buffered,
every later one is too. This removes the per-event `Box<Object>` clone in the
common steady-state case (frequent motion updates that process inline), the
avoidable allocation churn; a saturated rez burst still clones its overflow
(unavoidable to defer it).

## Original brief

Tracy self-time over the first ~10 s of rezzing on Aditi shows the render-world
prepare of our custom face material (`SlFaceExt`, an `ExtendedMaterial` over
`StandardMaterial`) running **every frame**:

| System | ms/frame | n |
| --- | --- | --- |
| `erased_render_asset::prepare_erased_assets<…SlFaceExt>` | 1.07 | 203 |
| `par_for_each … Changed<MeshMaterial3d<…SlFaceExt>>` | 0.29 | 3444 |
| `material::check_entities_needing_specialization<…SlFaceExt>` | 0.11 | 203 |

Some of this is legitimate while prims stream in (new materials genuinely need
preparing). But the concern is whether existing `SlFaceExt` materials are being
**mutated every frame**, which marks them `Changed` and forces a full
`ExtendedMaterial` bind-group re-prepare — the exact trap recorded in the
`sl-client-facematerial-no-per-frame-mutation` memory (ExtendedMaterial re-prep
= full bind-group recreate; time-based face effects must be driven GPU-side from
`globals.time`, with params written once on change).

Investigate:

- Whether any system writes `Assets<…SlFaceExt>` / the material component each
  frame (texture-anim faces, hover/glow, alpha, bump) rather than only on an
  actual state change. The `Changed<MeshMaterial3d<…SlFaceExt>>` count (3444
  hits over 203 frames ≈ 17 changed entities/frame) suggests steady churn even
  once a face is stable.
- Whether the prepare cost tracks the number of *changed* materials (expected
  during rez) or a fixed per-frame set (a mutation bug).

Measure with a ≤10 s `tracy-capture` capture while **stationary and fully
rezzed** — if the prepare stays ~1 ms/frame with nothing new rezzing, something
is mutating materials every frame.
