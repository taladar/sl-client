---
id: viewer-perf-gpu-avatar-crowd-cpu-bound
title: 100-avatar crowd frame is CPU/Main-schedule bound
topic: viewer
status: ready
origin: GPU-avatar Phase 5 crowd + culling measurement (2026-08-14)
refs: [viewer-perf-gpu-avatar-crowd, viewer-perf-gpu-avatar-phase5-lod-polish, viewer-perf-gpu-avatar-pose-lod]
---

Context: [context/gpu-avatars.md](../context/gpu-avatars.md).

Measured with `SL_VIEWER_CROWD=100` (tracy, post-spawn medians): **Main
schedule 71.7 ms > Render 48.7 ms**. The crowd frame is **CPU/Main bound**, not
draw bound. So the GPU-bounds frustum culling
([[viewer-perf-gpu-avatar-phase5-lod-polish]]) — which correctly drops the
off-screen draw ~20 ms (render_system 27 ms → 6 ms) — does **not** improve
crowd FPS: Render is already below Main, so culling the draw can't move the
frame. The visible crowd-FPS win is in **reducing Main**.

## Decompose Main (not yet done)

The known per-avatar Main-side costs don't obviously sum to ~72 ms — the biggest
unaccounted chunk is likely **PostUpdate transform propagation** over ~5000
crowd entities (100 avatars × body root + sockets + ~45 submeshes). Profile and
attribute Main at 100 avatars, then attack the top contributors. Candidates:

- **Transform propagation** over the crowd's ~5000 entities (sockets + submesh
  children). Do skinned submeshes even need per-frame propagation now that
  placement is the GPU pose root? (They carry the entity transform only for the
  Aabb round-trip / instancing.)
- **avian3d physics** — are the crowd copies (and real avatars) generating
  collider/rigid-body work per frame? Crowd copies should carry no physics.
- **`apply_gpu_avatar_bounds`** iterating every `GpuSkinBinding` entity each
  frame (~500) — batch / dirty-gate.
- **`stage_gpu_avatars`** per-slot feed (~3 ms, 100 slots) — the
  [[viewer-perf-gpu-avatar-pose-lod]] phase-bucket knob trims this for
  off-screen/far avatars (now more relevant than first thought).
- **`collect_pick_warm_set`** (~4 ms at 100) — see the pick-warm bug task.

## Verify

`CROWD=100`: Main median falls below Render, frame period drops, and *then* the
frustum culling's draw savings actually surface as FPS when looking away.
