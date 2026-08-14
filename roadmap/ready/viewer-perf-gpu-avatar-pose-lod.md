---
id: viewer-perf-gpu-avatar-pose-lod
title: GPU-avatar pose LOD — phase-bucket temporal + bone-count LOD
topic: viewer
status: ready
origin: GPU-avatar Phase 5 crowd measurement (2026-08-14)
refs: [viewer-perf-gpu-avatar-phase5-lod-polish, viewer-perf-animation-lod-pose-cache, viewer-perf-gpu-avatar-crowd]
---

Context: [context/gpu-avatars.md](../context/gpu-avatars.md) §8. Split out of
[[viewer-perf-gpu-avatar-phase5-lod-polish]] as its own follow-up after the
`CROWD=100` measurement **demoted** these knobs: the pose compute is already
negligible at 100 avatars (`run_gpu_avatar_compute` 0.16 ms/frame), so these
reduce **already-cheap** work. Do the draw-side levers first (GPU-bounds frustum
culling, mesh-LOD); this is the smaller, later cleanup for the pose residual.

## The two knobs

- **Phase-bucket temporal LOD**: per-avatar pose-recompute cadence by
  screen-space size × recency (the [[viewer-perf-animation-lod-pose-cache]]
  policy). Near avatars exact-phase; far / occluded ones bucketed at 15→10 Hz.
  Palettes persist between updates (skip the avatar's B/C/D rows via a dirty
  list), so a skipped avatar holds its last pose — the buffers' persistence
  gives the budget-pose-update for free. Trims the `stage_gpu_avatars` per-slot
  feed (~3 ms at 100) for off-screen/far avatars — the residual that frustum
  culling can't reach (the CPU feed runs in main-world `PostUpdate`, before the
  render world computes visibility, so gate on a **CPU-side screen-size
  estimate**, not `ViewVisibility`).
- **Bone-count LOD**: a reduced canonical level list (drop face / finger / wing
  joints) per avatar; passes B/C iterate the reduced list and pass C writes
  parent-chain results for skipped joints (inherit parent world) so palettes
  stay valid. No weight remap (weights reference canonical indices whose
  matrices are simply less fresh). Reduces the (already tiny) compute; mainly
  useful as extreme-crowd headroom.

Honor the memory lesson: throttle **pose recompute**, never render-resource
cadence (the probe-cadence frame-spike trap — a memory).

## Verify

With the crowd harness (`SL_VIEWER_CROWD=N`): far/occluded copies update pose at
the bucketed cadence (no visible stutter), `stage_gpu_avatars` per-frame cost
falls with distance/occlusion, near copies stay exact-phase. Bone-count LOD:
distant copies use the reduced joint list with no perceptible skinning change.
