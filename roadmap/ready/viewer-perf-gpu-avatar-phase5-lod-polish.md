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

## Progress + measured direction (2026-08-14)

**Step 1 — synthetic-crowd harness landed** (`SL_VIEWER_CROWD=N`, module
`gpu_avatars/crowd.rs`): N GPU-instanced copies of the local avatar, same-body
handles (so they batch), a golden-ratio phase offset + rate jitter per copy
(realistic desync, and it exercises the phase-bucket LOD), a `⌈√N⌉²` grid, and
settle-detection so it captures the avatar's *final* baked draw set (BOM
body/head included, system body excluded). No-op when unset. This is the
measurement/verification tool for the rest of Phase 5.

**What the `CROWD=100` measurement changed:** the **pose compute is negligible
at 100 avatars** (`run_gpu_avatar_compute` 0.16 ms, `stage_gpu_avatars` ~3 ms),
so the originally-headline pose-LOD knobs (phase-bucket coarsening, bone-count
LOD) target already-cheap work — **demoted to small follow-ups** (phase-bucket
still worth ~the stage residual for off-screen avatars). The crowd cost is the
**draw + Bevy per-visible-skin extract**. So the **first knob is the
GPU-computed skinned bounds → retire `NoFrustumCulling`** (removes off-screen
avatars from draw + `extract_skins`, both `ViewVisibility`-gated) — scoped as
"compute correct posed bounds first, then enable culling", since the current
`NoFrustumCulling` is load-bearing (bind-pose/dummy-joint bounds are
meaningless). A large **on-screen** crowd is draw-bound in a way culling can't
help (they're all visible) → separate **mesh-LOD** task filed
([[viewer-perf-avatar-mesh-lod-screen-size]]); impostors
([[viewer-avatar-impostors-billboard]]) stay the extreme fallback. Also noted:
`collect_pick_warm_set` scales with the crowd (~4 ms at 100) — the pick pre-warm
touching un-pickable copies, a bug to fix.
