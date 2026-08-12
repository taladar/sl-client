---
id: viewer-perf-gpu-avatar-phase1-gpu-fk-palettes
title: GPU avatars Phase 1 — GPU FK + palettes (kills the serial extract)
topic: viewer
status: blocked
origin: GPU-avatar design (2026-08-12), context/gpu-avatars.md §2, §7 Phase 1
refs: [viewer-perf-gpu-avatar-crowd, viewer-perf-avatar-pose-extract-skins]
blocked_by: [viewer-perf-gpu-avatar-phase0-mesh-dedup, viewer-perf-gpu-avatar-keystone-skinuniforms-spike]
---

Context: [context/gpu-avatars.md](../context/gpu-avatars.md) §1.2(c/e/f), §2
(passes C+D, §2.3 scheduling, §2.4 write-in), §7 "Phase 1". Epic:
[[viewer-perf-gpu-avatar-crowd]].

Land the buffers (rest / frame / working, minus clips), compute **pass C**
(hierarchical FK) and **pass D** (skin palettes → `SkinUniforms`), the `Core3d`
compute system, and the `SkinUniforms` write-in (per the keystone spike
outcome). CPU still runs `resolve_pose` + adjusters, but instead of
`write_joint_globals` it uploads the blended `LocalPose` rows and skips passes
A/B. Joint entities stay spawned but **frozen** (never written) so Bevy's
`extract_skins` sees no `Changed<GlobalTransform>` and the serial extract
collapses. **Socket-joint CPU mini-FK** (§5.4) lands here — rigid attachments
can no longer read posed joint globals, so sockets write a `Transform` under
the avatar root (this also deletes the `pose_attachment_nodes` re-propagation
and the orphaned-children bug class).

Behind `SL_VIEWER_GPU_AVATARS` (default off; runtime capability check); the CPU
path stays fully intact underneath. Verify: screenshot A/B GPU vs CPU (T-pose
harness + animated); Tracy A/B shows `ExtractSchedule` median → `extract_lights`
level and the PostUpdate joint-propagation cost gone; attachments / name tags /
camera all still track. This is the phase that turns the ~7 ms serial
`extract_skins` ([[viewer-perf-avatar-pose-extract-skins]]) into byte-sized
delta uploads.

## Constraints from the keystone spike (confirmed 2026-08-12)

The `SkinUniforms.current_buffer` write-in is **proven** on a real ~106-joint
mesh-body avatar (bit-exact, 89/89 `write LANDED`, no validation error) — see
[[viewer-perf-gpu-avatar-keystone-skinuniforms-spike]]. Reuse its binding /
scheduling / readback code (the committed `gpu_avatar_spike` diagnostic,
`SL_VIEWER_GPU_AVATAR_SPIKE`) as the Phase 1 starting point, and honor:

- **`current_buffer` has no `COPY_SRC`** (`STORAGE | COPY_DST`) — the
  `SL_VIEWER_GPU_AVATARS_VALIDATE` palette readback must be a **compute copy**
  through the storage binding, not `copy_buffer_to_buffer`.
- **`current_skin_index` staleness** — Bevy bakes the skin offset into the mesh
  instance uniform only when the instance re-extracts; a fully static skinned
  mesh can sit at `u32::MAX` and render nothing. Real avatars re-extract every
  frame (moving), so Phase 1 is safe, but the Phase 4 frozen-skin endgame must
  guard against it (keep instances re-extracting, or force the index).
