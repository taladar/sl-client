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

## Verify

Tracy on a representative + a crowd scene; attribute `camera_driver` time to
render-graph nodes (RenderDoc for pass-level GPU cost); pick the biggest lever
and re-measure the frame period. Separate task tracks the outlier spikes
([[viewer-perf-asset-streaming-frame-spikes]]); this one is the steady-state
average frame.
