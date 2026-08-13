---
id: viewer-perf-gpu-avatar-phase2-gpu-sample-blend
title: GPU avatars Phase 2 — GPU clip sample + priority/ease blend
topic: viewer
status: ready
origin: GPU-avatar design (2026-08-12), context/gpu-avatars.md §2.2, §7 Phase 2
refs: [viewer-perf-gpu-avatar-crowd, viewer-perf-animation-lod-pose-cache]
blocked_by: [viewer-perf-gpu-avatar-phase1-gpu-fk-palettes]
---

Context: [context/gpu-avatars.md](../context/gpu-avatars.md) §1.2(a), §1.3(d),
§2.1–§2.2 (passes A+B), §3.4, §7 "Phase 2". Epic:
[[viewer-perf-gpu-avatar-crowd]].

Move the sample+blend fold onto the GPU: clip upload on `.anim` decode; the
playback + correction buffers; **pass A** (clip sample, deduplicated by
`(clip, phase_bucket)` — the animation-data instancing) and **pass B**
(per-joint priority/ease blend + idle adjusters + sparse CPU corrections). CPU
`resolve_pose` is demoted to the ~25-joint adjuster mini-pose only (§5.3). The
`(clip, phase)` pose-cache dedup is the CPU scheduler in §2.1 — 40 synced
dancers collapse to one sample job (ties to
[[viewer-perf-animation-lod-pose-cache]]).

Verify: WGSL-vs-Rust golden tests for `sample_motion` / `blend_joint` /
`pose_weight` (loop wrap, quat nlerp short-arc, priority ties, recency, 4-slot
cap, weight-budget fold, ease cubic) — §9.2; dance-club A/B where the
`pose_avatar_skeletons` successor is scheduling-only; a long soak for
playback-clock drift (walk-speed skew, loop wrap).
