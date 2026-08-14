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

**Step 2 — GPU-bounds frustum culling landed + verified (2026-08-14).** Pass D
emits a per-instance world AABB (reduction over posed joint positions +
conservative 1.25 m margin); an async readback feeds it back as each
avatar/crowd submesh's `Aabb` (before Bevy's `CalculateBounds`), and
`NoFrustumCulling` is removed on the avatar path only (bind-pose/dummy-joint
bounds were meaningless — so this is "compute correct posed bounds, then cull",
not a bare delete). Verified: `ViewVisibility` census 0/506 with the crowd
off-screen, `render_system` drops 27 ms → 6 ms off-screen, a headless cull test,
zoom-in-no-vanish. Crash fixed along the way (a bounds/palettes
bind-group-layout mismatch; separate bounds layout stays at 3 buffers — the
shared pose layout is pinned at the 8- storage-buffer downlevel floor — and the
pose bind group is re-bound before the palettes dispatch).

**Key measured re-prioritization:** culling is correct + necessary but does
**not** improve crowd FPS yet, because the 100-avatar frame is
**CPU/Main bound** (Main 71.7 ms > Render 48.7 ms) — culling only cuts the
already-smaller Render. The visible crowd-FPS win now lives in the CPU
follow-ups: [[viewer-perf-gpu-avatar-crowd-cpu-bound]] (decompose Main),
[[viewer-perf-gpu-avatar-extract-skins-floor]] (the non-cullable `extract_skins`
iterate-all floor), [[viewer-perf-pick-warm-set-scales-with-crowd]], and the
pose-LOD [[viewer-perf-gpu-avatar-pose-lod]] (trims `stage_gpu_avatars` in Main
— now more relevant than first thought). Harness cosmetic:
[[viewer-crowd-harness-copies-render-grey]].
