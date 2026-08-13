---
id: viewer-perf-frame-work-budget-priorities
title: Frame-work budget arbiter with priorities (skip low-priority work)
topic: viewer
status: ideas
origin: GPU-avatar Phase 4 perf discussion (2026-08-13)
refs: [viewer-perf-asset-streaming-frame-spikes, viewer-perf-render-app-bound-frame, viewer-perf-animation-lod-pose-cache]
---

Context: [context/viewer.md](../context/viewer.md).

A level above per-subsystem budgeting
([[viewer-perf-asset-streaming-frame-spikes]] amortizes one subsystem across
frames): a **frame-wide work-budget arbiter with priorities**. Several
subsystems each want to do deferrable, budgetable work each frame — texture
upload/apply, mesh upload/allocation, pose recompute, reflection-probe capture,
env-map generation, pick-pipeline warm, etc. Today each throttles in isolation,
so a frame can still stack several budgets and stall, and a low-priority
subsystem can spend budget while a high-priority one has urgent work queued.

## The idea

A single per-frame budget (wall-clock or a proxy) allocated by **priority**:
subsystems register their pending deferrable work with a priority; the arbiter
spends the frame's budget on the highest-priority pending work first and
**skips lower-priority budgeted work entirely** in a frame where higher-priority
work needs it — not merely throttling everyone proportionally. Skipped work
rolls to a later frame. So e.g. a frame busy uploading textures the user is
about to see does **not** also run a background env-probe refresh or a distant
pose recompute; those wait for a calmer frame.

## Why it's more than the sibling budgeting task

Per-subsystem budgeting bounds *each* subsystem's per-frame slice; it does not
coordinate *across* them. Two independently-bounded subsystems that both fire
on the same frame still add up. Priority arbitration makes that trade-off
explicit and dynamic: important-now work preempts deferrable-background work
within one shared budget.

## Open questions (why this is an idea, not ready)

- The budget currency: wall-clock estimate per unit of work (needs per-unit
  cost estimates), or a simpler count/byte proxy per subsystem normalized to a
  shared scale?
- Static priority tiers vs a dynamic priority (screen-space size / recency /
  user focus — the animation-LOD policy already thinks this way for pose
  recompute, [[viewer-perf-animation-lod-pose-cache]]).
- Starvation guard: a low-priority task perpetually skipped in a busy scene
  needs a floor / aging so it eventually runs.
- Where it lives: a main-world arbiter resource the budgeted systems query, vs
  a render-world one — some budgeted work is render-side (uploads).

Do the concrete per-subsystem budgets first; this arbiter coordinates them once
there is more than one and they demonstrably collide.
