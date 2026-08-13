---
id: viewer-perf-pipeline-prewarm-during-login
title: Pre-warm render pipelines during the login/connect window
topic: viewer
status: ideas
origin: user idea during GPU-avatar Phase 3 (2026-08-13)
refs:
  - viewer-perf-pipeline-specialization-stalls
  - viewer-p1-1-login-from-credentials
  - viewer-perf-gpu-avatar-phase3-gpu-picking
---

Context: [context/viewer.md](../context/viewer.md).

**Tier 1 done (2026-08-13):** the custom pick pipelines are pre-warmed —
`collect_pick_warm_set` → `warm_gpu_pick_pipelines` specializes both
`GpuPickKey` variants against each pickable mesh's real layout as it rezzes,
so the async compile runs before the first pick (no more first-pick miss).
Shipped with [[viewer-perf-gpu-avatar-phase3-gpu-picking]]. The gpu_avatars
compute passes (A–D) were already queued eagerly at `RenderStartup`. **Tier 2**
(the Bevy material/mesh warm-up scene) remains — it pairs with the login-screen
UI work.

Use the pre-world-render window — a real **login screen** (up for several
seconds) or, even today, the login-XML-RPC + region-handshake + initial-stream
period — to **front-load pipeline compilation** so pipelines are *ready* before
first use, removing the "not ready for the first few frames" latency (the
Phase 3 first-pick miss; materials/objects popping in slightly late on first
draw).

## Established facts (from [[viewer-perf-pipeline-specialization-stalls]])

- **Compilation is already async** on this build: avian pulls
  `bevy/multi_threaded`, so Bevy compiles pipelines on the
  `AsyncComputeTaskPool` (`synchronous_pipeline_compilation = false`) — a
  first-use compile does **not** hard-stall the frame thread; the symptom is
  the pipeline's output not being *ready* for a few frames.
- **No disk pipeline cache** in Bevy 0.19 / this wgpu (`create_pipeline_cache`
  absent) → no cross-run persistence. Pre-warm is **per-run** — it only
  front-loads the compile into the login window, it can't skip it across runs.

So the win is latency-hiding, not compile-elimination: kick the async compiles
off early (during login) so they finish before the world/cursor needs them.

## Two tiers

1. **Custom pipelines (easy, high-value):** the gpu_avatars compute passes
   (A–D) and the gpu_pick static/skinned pipelines — we own the descriptors, so
   **queue them into `PipelineCache` at `RenderStartup`** (or on entering the
   connecting state) rather than lazily on first dispatch/draw. The async
   compile then runs during login; kills the Phase 3 first-pick miss and the
   first-frame compute latency. Small change.
2. **Bevy material/mesh pipelines (more work):** each `FaceMaterial` /
   `StandardMaterial` / sky / water / terrain / name-tag variant, static and
   **skinned**, specializes on first *draw*. Pre-warm by rendering a hidden
   **warm-up scene** during login that draws one instance of each variant into
   an offscreen target, forcing `PipelineCache` to queue+compile them before
   the real world renders. The work is enumerating the variants (and keeping
   the list in sync as materials are added).

## Dependency / sequencing

A real login screen gives the most (a deterministic multi-second window); the
current connect/handshake period offers a shorter one. Tier 1 is worth doing
regardless (front-load at `RenderStartup`). Tier 2 pairs naturally with the
login-screen UI work. Verify by Tracy/first-frame: no first-pick miss, no
visible first-draw material pop, pipelines already `Ok` in `PipelineCache` by
the time the world rezzes.
