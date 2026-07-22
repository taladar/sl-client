---
id: viewer-perf-skeleton-single-solve
title: Solve each avatar skeleton once per frame unless an adjuster needs two
topic: viewer
status: ideas
origin: performance survey of the implemented viewer (2026-07-22)
refs: [viewer-profiling, viewer-avatar-impostors-billboard]
---

Context: [context/viewer.md](../context/viewer.md).

`pose_avatar_skeletons` (`animations.rs:961-1259`) runs the full SL
skeletal recurrence — per-joint world-matrix chain over ~30+ joints plus
collision-volume deforms (`skeleton.deformed_world_matrices`) — **twice
per avatar per frame**: once at `animations.rs:1073` producing `world0`,
and again at `animations.rs:1249` producing the final `world` (a third
time at `1198` only under `log_ik`).

Why `world0` exists (so the implementer does not re-derive it): it is the
**pre-adjustment joint geometry** that the pose *adjusters* read as input
— look-at (`1076`/`1089`, only when the avatar has a look target), reach
/ point-at (`1121-1127`), locomotion IK (`1156-1185`, mainly the moving
own avatar), and body physics (`1223-1235`, only when enabled and
active). After those folds are applied, the second pass computes the
definitive matrices.

Explicitly checked and NOT a consumer: the pixel-perfect avatar click
pick (`avatar_pick.rs`) reproduces GPU skinning on the CPU **on demand
only** — when a pick is requested (right-click / debug inspector), behind
a broad-phase box test — reading joint `GlobalTransform`s directly. It
contributes nothing per frame and does not read `world0`, so gating
`world0` cannot break picking.

## Proposed fix

Compute `world0` only when at least one adjuster is actually active for
that avatar:

```text
needs_world0 = look_targets.point(agent).is_some()
  || point_at.is_some() || is_aiming
  || locomotion IK active (moving, grounded own/near avatar)
  || body physics enabled && active
```

Otherwise fold the idle pose and run a single final
`deformed_world_matrices` pass. For a crowd of distant idle avatars —
the common busy-club case — every avatar drops to one solve.

## Estimated impact

High: up to ~50% of skeletal-solve CPU for idle/distant avatars, scaling
linearly with avatar count. Skeletal solve is a leading per-avatar CPU
cost, so in a 20+ avatar scene this is likely one of the largest single
wins available (same scaling regime [[viewer-avatar-impostors-billboard]]
attacks from the render side; the two compose). Measure per-system
self-time with [[viewer-profiling]] Tracy before/after in a crowd scene.

Confidence: high on the double solve and on which folds consume `world0`
(all call sites verified); the exact activity predicate needs care so an
adjuster never reads a missing `world0` (fall back to computing it if in
doubt — the gate is an optimization, not a behaviour change).
