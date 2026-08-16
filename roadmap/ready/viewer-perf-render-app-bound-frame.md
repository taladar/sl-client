---
id: viewer-perf-render-app-bound-frame
title: Average frame is now render-app bound (render graph / GPU draw)
topic: viewer
status: ready
origin: GPU-avatar Phase 4 perf capture (2026-08-13)
refs: [viewer-perf-gpu-avatar-phase4-remove-scaffolding, viewer-perf-gpu-avatar-crowd]
---

Context: [context/viewer.md](../context/viewer.md).

With the avatar posing/extract cost removed (Phase 4), the **average** frame is
now render-app bound. From a 27.6 s / 512-frame aditi capture (~27 fps), Main
and RenderApp both run ~36 ms and pipeline, so the frame period is the render
app. Inside it (mean/frame):

- `bevy_render::renderer::render_system` — **18.1 ms** (runs the render graph),
  of which `core_pipeline::camera_driver` / `RenderGraph` (the actual draw
  passes / GPU) is **~11–16 ms**.
- `submit_pending_command_buffers` — 4.5 ms.
- the extract phase — 5.2 ms (`extract_skins` now only 0.76 ms of it — the
  Phase 4 win; other extract/prepare systems dominate it now).

Avatar work is no longer a frame-time factor. The new ceiling is render-graph
execution + GPU draw + render-side prepare/queue.

## Candidate levers (investigate before committing to one)

- **Draw/GPU** (`camera_driver` ~11–16 ms): batching / draw-call count,
  overdraw (transparent passes), shadow passes (`queue_shadows` /
  `specialize_shadows` show up), local-light clustering
  (`prepare_clusters_for_gpu`), env-probe generation
  (`prepare_generated_environment_map`) — profile which passes own the draw
  time on a representative scene.
- **Render-side CPU prepare/queue**: `specialize_shadows`,
  `check_visibility_cpu_culling`, `prepare_preprocess_bind_groups`,
  `collect_pick_warm_set` (0.9 ms/frame — the Phase 3 pre-warm; check it is not
  over-collecting) — trim per-frame CPU work that doesn't change.
- **Main side** (~36 ms, pipelined so only matters if it becomes the longer
  leg): physics (`run_fixed_main_schedule` 7.7 ms, avian substeps),
  `PostUpdate` propagation.

## Re-capture 2026-08-15 (aditi, denser scene) — render-bound confirmed, heavier

A fresh full-session `tracy-capture` (release, `profile-tracy`,
`RUST_LOG=warn,bevy_ecs=info,bevy_render=info`, 1890 frames / 2:18, 13.7 M
zones, clean disconnect; trace `tracy-captures/aditi-2026-08-15.tracy`)
re-measures the average frame on a **denser aditi spot** than the 2026-08-13
27-fps capture. 28 frames were compositor-throttled (~954 ms `present`, all in
the first ~30 s while the window was unfocused during login/rez) and are
excluded; all numbers are the visible steady state (finished-frame deltas):

- frame: mean **57.1 ms (17.5 fps)**, p50 **53.4 ms**, p95 84.1, p99 127.5,
  max 379.9.
- `schedule{name=Render}` (thread 2, concurrent) mean **52.6 ms**,
  corr(frame) **0.92**, present only 0.13 ms — **this is the gate.**
- `schedule{name=Main}` (thread 1) mean **34.1 ms** — runs concurrently and
  fits under Render, so the frame is squarely **render-app bound** (unlike the
  2026-08-12 co-limited local/aditi captures).

Render splits into render-side prepare/queue (~22 ms) + **`render_system`
(render graph) 30.9 ms** (p95 45.7). That render_system is **up from 18.1 ms**
on the 2026-08-13 baseline — the render graph / draw is the dominant lever, and
the scene renders **~8.76 `Core3d` passes per frame** (main view + the GPU-pick
view + atmosphere env-map + reflection/light-probe cubemap faces), each a full
deferred prepass + opaque + transparent + OIT + AA + tonemapping. Attribute
`camera_driver` to specific nodes next (many-views × many-passes is the
multiplier). Caveat: this spot is denser / has more reflective content than the
2026-08-13 run, so part of the 18→31 ms rise is scene conditions — the
qualitative finding (render-bound, render_system dominant, probe/env multi-pass)
is what holds.

Main (34 ms, non-gating here) = PostUpdate 14.4 + Update 11.4 + FixedLoop 5.0 +
Extract 3.5 + PreUpdate 2.2. On the "render-side CPU prepare/queue" lever above:
**`collect_pick_warm_set` is now 3.77 ms/frame** (the top steady `Update`
system, with only the primary avatar) — it *is* over-collecting, but on a dense
**prim** region rather than a crowd; see
[[viewer-perf-pick-warm-set-scales-with-crowd]]. `build_static_colliders`
(1.16 ms/frame, full O(all prims) scan) is the next viewer-side steady cost.

## Re-capture 2026-08-16 — the render leg is CPU-bound, not GPU

Fresh captures after avian removal ([[viewer-perf-custom-static-raycast-index]])
and the change-driven pick warm
([[viewer-perf-pick-warm-set-scales-with-crowd]]) —
`tracy-captures/aditi-2026-08-16-postpickwarm.tracy`, GPU-zone track included.
Two corrections to the earlier framing:

**1. Both legs are over budget — "render-bound so Main doesn't matter" was
wrong.** Main is down to **~25.7 ms** (First→Last), Render ~**50 ms**; the
median frame is still ~53 ms. Render is the *longer* leg, but Main is itself
~1.5× the 16.6 ms/60 fps budget, so reaching 60 fps needs both to come down —
`frame ≈ extract + max(Main, Render)`, and cutting only the longer floors at
`extract + the other`.

**2. The render leg is ~90 % CPU, not GPU.** Summing the Tracy GPU-zone track:
**total GPU execution is only ~4.3 ms/frame** (opaque 2.2, clustering 0.6,
transparent **0.5**, msaa_writeback 0.4, upscaling 0.3, …) against a ~50 ms
render leg — the RX 7900 XTX is ~92 % idle. So the cost is **CPU command
encoding + submission**, and no shader/GPU optimization moves it. CPU render-
thread costs (self-time/frame):

| zone | CPU/frame | GPU/frame | nature |
| --- | --- | --- | --- |
| `submit_pending_command_buffers` | **6.0 ms** | — | per **view × pass** command-buffer finalize + `vkQueueSubmit`; not per-entity |
| `main_transparent_pass_3d` (×~15 views) | **4.1 ms** | 0.5 ms | **8× CPU-bound** — per-item draw encode; transparent can't batch (depth-sorted) |
| `main_opaque_pass_3d` (×~15) | 1.9 ms | 2.2 ms | the one GPU/CPU-balanced pass |
| render-world prepare/queue | ~21 ms | — | per-object / per-view: clustering 2.0, queue/collect/specialize ~1 each, `gpu_preprocess`/culling heavily parallel |

Draw batching already works (meshes deduped via `GeometryCache` + `MeshStore`,
materials interned — [[viewer-perf-material-intern]]) — which is *why* GPU draw
time is tiny. Batching cuts **draw calls**, but the render-thread cost is
**command-buffer count (per view×pass)** + **per-entity prepare/queue** + the
**un-batchable transparent** path, none of which asset dedup touches.

### Which levers are viable

- **NOT viable — transparent item count**: user-generated SL content; we cannot
  reduce how many transparent faces a region has, and Bevy can't batch them.
- **NOT viable — fewer views / probe cadence**: must stay SL-faithful, and
  throttling probe capture caused large spikes (Bevy discards unused
  pipeline/mesh caches — the probe-cadence lesson). `submit` (6 ms) is bound to
  view×pass, so it is effectively fixed.
- **The remaining lever — entity-count reduction**: collapse a prim's per-face
  entities into one per-object mesh, cutting the per-entity *serial* render work
  (queue/collect/phase-item build) and the main-thread PostUpdate visibility
  ~4×. Ceiling is the per-entity serial cost (order ~5–10 ms, partly on the
  non-gating Main leg), not the submit/transparent head — measure first. Tracked
  in [[viewer-perf-per-object-face-merge-entity-count]] (revived 2026-08-16).

## Verify

Tracy on a representative + a crowd scene; the render leg is CPU-bound (confirm
via the GPU-zone track staying ~4 ms), so measure CPU submit/encode + per-entity
prepare/queue, not GPU pass time. Separate task tracks the outlier spikes
([[viewer-perf-asset-streaming-frame-spikes]]); this one is the steady-state
average frame.
