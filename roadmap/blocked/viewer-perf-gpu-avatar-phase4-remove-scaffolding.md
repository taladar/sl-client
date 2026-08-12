---
id: viewer-perf-gpu-avatar-phase4-remove-scaffolding
title: GPU avatars Phase 4 — remove joint entities + CPU pose scaffolding
topic: viewer
status: blocked
origin: GPU-avatar design (2026-08-12), context/gpu-avatars.md §7 Phase 4
refs: [viewer-perf-gpu-avatar-crowd]
blocked_by: [viewer-perf-gpu-avatar-phase2-gpu-sample-blend, viewer-perf-gpu-avatar-phase3-gpu-picking]
---

Context: [context/gpu-avatars.md](../context/gpu-avatars.md) §5 (leaves-the-CPU
list), §7 "Phase 4", Appendix A. Epic: [[viewer-perf-gpu-avatar-crowd]].

Remove the transitional scaffolding once GPU pose + GPU picking are both live:
replace per-avatar joint-entity spawning with slot registration; register
avatar skins in `SkinUniforms` via the **frozen dummy-joint trick** (or,
attempted first, an upstream Bevy PR adding an "externally-written skin" marker
so the staging bytes disappear too). Delete `pose_avatar_skeletons`,
`write_joint_globals`, `pose_attachment_nodes`, `PoseGate`, and the joint-entity
spawn path (the full-skeleton `deformed_world_matrices` stays for
rest-pose/one-shot uses: spawn placement, body metrics). Migrate **animesh**
control avatars onto avatar slots (same machinery, `ObjectPlayingAnimation`
source). Flip `SL_VIEWER_GPU_AVATARS` from default-off to a downlevel-fallback
selector.

Verify: no `Changed<GlobalTransform>` avatar joints remain (frozen-joint
extract residue → 0); animesh renders identically; the whole-arc acceptance test
(§9.2) — dance-club frame no longer co-limited by avatar work.
