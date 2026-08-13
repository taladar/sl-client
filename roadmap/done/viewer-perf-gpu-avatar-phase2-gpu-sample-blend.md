---
id: viewer-perf-gpu-avatar-phase2-gpu-sample-blend
title: GPU avatars Phase 2 — GPU clip sample + priority/ease blend
topic: viewer
status: done
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

## DONE (2026-08-13)

Landed and committed. Clip upload (`ClipArena`, dedup by asset, keyframes exact,
names → canonical), per-avatar playback (`GpuPlayState[16]`, uploaded only on
content change — steady-state loops and walking avatars upload zero playback
bytes), pass A (sample) + pass B (blend + idle + sparse corrections) in
`pose.wgsl`, and the CPU demotion: `resolve_pose` restricted to the per-avatar
**mini-pose subset** (adjuster chains + collision volumes + sockets + ancestor
closure), `pose_avatar_skeletons` publishing only the **diffed** channels its
adjusters changed as sparse corrections. The `(clip, phase_bucket)` sample-job
dedup collapses synced dancers to one job (`anim_offset` folded into `phase`
CPU-side). The readback verdict's CPU reference now runs the **whole** mirror
(`mirror_local_pose` → `reference_fk` → ×IBP), so `GPU==CPU` isolates
sample/blend faults too.

Verified: passes A+B are **bit-exact** vs `sl_anim` across all the SL quirks + a
3000-step loop-wrap soak; headless GPU end-to-end ≤1e-4; 27 gpu_avatars tests +
75/75 sl-client-bevy, clippy clean. **Live aditi** (tracy, readback on): 150
`GPU palette == CPU palette` (worst 6.1e-5, 87-joint mesh body) + user visual
pass (avatars animate, IK/look-at/attachments track). `=cpu` stays byte-for-byte
legacy; ghost mode keeps the Phase 1 CPU-blend split. **Deferred:** animesh
stays CPU (Phase 4); the dance-club perf A/B (`pose_avatar_skeletons` ≈
scheduling-only) still wants a crowd scene.
