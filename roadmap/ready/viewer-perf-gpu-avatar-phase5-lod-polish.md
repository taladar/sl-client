---
id: viewer-perf-gpu-avatar-phase5-lod-polish
title: GPU avatars Phase 5 — LOD, scalability hooks, polish
topic: viewer
status: ready
origin: GPU-avatar design (2026-08-12), context/gpu-avatars.md §8, §7 Phase 5
refs: [viewer-perf-gpu-avatar-crowd, viewer-perf-animation-lod-pose-cache, viewer-avatar-impostors-billboard]
---

Context: [context/gpu-avatars.md](../context/gpu-avatars.md) §8, §7 "Phase 5".
Epic: [[viewer-perf-gpu-avatar-crowd]].

Scalability knobs, all data-driven from the CPU scheduler (the GPU passes just
get fed less): **phase-bucket coarsening** (temporal LOD by screen-size ×
recency; palettes persist between updates — the budget-pose-update from
[[viewer-perf-animation-lod-pose-cache]] for free), **bone-count LOD** (reduced
canonical level list; skipped joints inherit parent world — no weight remap
needed), a **MAX_ACTIVE clamp / priority floor** for background avatars, and
GPU-computed skinned bounds to retire `NoFrustumCulling`. Optional: box-select,
a second tiny HUD pick view. Impostors ([[viewer-avatar-impostors-billboard]])
stay deferred — this phase keeps full geometry viable much further out, so
impostors remain only the opt-in extreme-count / low-end fallback.

Honor the memory lesson: throttle **pose recompute**, never render-resource
cadence (the probe-cadence frame-spike trap).
