---
id: viewer-perf-avatar-pose-extract-skins
title: Avatar pose path drives extract_skins — the serial critical-path cost
topic: viewer
status: ideas
origin: Tracy critical-path reconstruction + code audit (2026-08-12)
refs: [viewer-perf-steady-state-46fps-ceiling, viewer-perf-skeleton-single-solve, viewer-perf-gpu-avatar-crowd, viewer-perf-animation-lod-pose-cache]
---

Context: [context/viewer.md](../context/viewer.md).

> **Scope correction (2026-08-12): this item is the small, test-scene win.**
> The idle-avatar framing below is biased by our (mostly-static) test scene.
> **Real avatars are almost never idle** — they run AO stands, dances, or sit
> loops, so most joints genuinely change every frame and the 15 Hz idle-wake is
> moot (the animation wakes them anyway). The per-joint `set_if_neq` skip below
> then saves only ~2–4× (the joints an animation does *not* move: face during a
> body dance, fingers during a walk, legs during a sit) — still worth doing
> (cheap, no divergence), but **not** most of the win. The real target is the
> **animated crowd / dance club**, where per-joint-skip AND distance-LOD both
> fail; the structural answer is [[viewer-perf-gpu-avatar-crowd]] and the
> near-term stopgaps are [[viewer-perf-animation-lod-pose-cache]]. Because Bevy
> 0.19's `extract_skins` is already incremental (keyed on `Changed<Global
> Transform>` into a persistent double-buffered joint buffer), this item's job
> is simply to **stop over-dirtying** — it does not add GPU work.

## Why this is the top lever

The 2026-08-12 aditi critical-path reconstruction
([[viewer-perf-steady-state-46fps-ceiling]]) shows the frame is
**co-limited main/render plus a fully serial `ExtractSchedule` (~7 ms)** — a
hard pipeline sync where **both threads are idle** and the main thread just
copies the world to the render world. That 7 ms is ~90 % **one system**:
`extract_skins` (5.4–7.6 ms), which extracts every skinned avatar's joint
matrices. Because it is on the serial segment, **every ms cut off it comes
straight off the frame** — the best ms-for-ms target in the whole frame.

`extract_skins` is a Bevy system, but its cost is **driven by our code**: how
many `SkinnedMesh` joints we mark changed each frame. Two viewer-side causes
inflate it, and the surrounding pose path also allocates heavily.

## Root causes (verified in source)

### 1. `write_joint_globals` rewrites all ~130 joints unconditionally

`animations.rs:1691` (writes at `:1705`/`:1716`). On each pose evaluation it
does `*global = GlobalTransform::from(..)` for **every** joint and rigid base
part with **no per-joint `set_if_neq`**, so the whole `SkinnedMesh` joint set
is marked changed even where the composed matrix is numerically identical —
and `extract_skins` then re-extracts all of them. Worse, the `PoseGate`
(`animations.rs:1298`) that skips *settled* avatars folds
`idle_now = (now*15).floor()/15` into its stamp (`:999`, `:1222`), so a fully
idle standing avatar **wakes 15×/sec** for procedural breathe/sway — and each
wake rewrites all joints. Fix: per-joint equality-skip in
`write_joint_globals` (skip the dirty when the matrix rounds equal to the
stored global), and/or lower the idle re-pose Hz. Idle joints are ~identical
between many 15 Hz ticks; moving avatars genuinely change. (Mind the memory
notes `sl-client-pose-driver-orphans-joint-children` and
`sl-client-bevy-change-detection-gotchas` when touching joint globals —
PostUpdate propagation stomps hand-written joint globals.)

### 2. Per-frame allocation on the pose path (multiplies by evaluated avatars)

All on `pose_avatar_skeletons` → per **evaluated** (moving/animating) avatar
per frame, the exact hot case in a populated region:

- `deformed_world_matrices` (`sl-client-bevy/src/avatars.rs:599`) allocates
  **four** fresh `Vec`s (`with_capacity` ≈ 130 joints) on every call, called
  **2×/avatar/frame** plus per animesh object (`animesh.rs:399`). Add a
  `deformed_world_matrices_into(&mut scratch)` variant; the three internal
  scratch Vecs become a cleared `Local<…>`, and the returned `world` Vec a
  reused `Local<Vec<Mat4>>` (each caller consumes it immediately). Highest-
  value alloc fix; helps both avatar and animesh paths.
- `effective_joint_overrides` (`avatars.rs:2452`) builds a temp
  `Vec<(&Uuid,&JointOverrides)>`, sorts it, then allocates a **new
  `JointOverrides` HashMap and merges into it every frame**; mesh-body avatars
  always have overrides. Cache the merged result and invalidate on the
  existing `pose_inputs_generation()` counter (`avatars.rs:2444`
  `bump_pose_inputs`) — the invalidation hook already exists.
- `pose_avatar_skeletons` (`animations.rs:1347`)
  `playback.poses.get(&agent).cloned()` clones an `AnimationPose` (two
  HashMaps) per avatar per frame; use a `Local<AnimationPose>` + `clone_from`
  to reuse the map allocations.

### 3. Uncached `std::env::var` in the pose hot loop (~6 syscalls/frame)

`pose_avatar_skeletons` (`animations.rs:1193-1196`, `1219-1220`) calls
`reach::log_enabled()`, `body_physics::log_enabled()`,
`look_at::LookAtDebug::from_env()`, `pose_gate_enabled()`, `t_pose_enabled()`,
`locomotion_ik::log_enabled()` every frame — each a fresh `std::env::var(..)`.
Cache in a `Local<Option<bool>>` at first use (pattern:
`particles.rs:971`'s `hud_disabled` Local). Cheap individually, strictly
redundant, and reads like an accidental hot-loop syscall.

## Feasibility / verification

Root cause #1 is the real frame lever (cuts the serial `extract_skins`); #2/#3
are allocation-hum and syscall cleanups on the same path (measure in Tracy
memory mode + `extract_skins` mean). A/B: several avatars **standing idle**
(the 15 Hz-wake case) vs the same scene with the per-joint skip — expect the
`ExtractSchedule` median to drop toward `extract_lights`-only. Confirm no
regression to animated avatars (they must still update). Related but distinct:
[[viewer-perf-skeleton-single-solve]] (solve once, not per consumer).
