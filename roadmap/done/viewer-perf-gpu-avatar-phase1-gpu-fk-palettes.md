---
id: viewer-perf-gpu-avatar-phase1-gpu-fk-palettes
title: GPU avatars Phase 1 — GPU FK + palettes (kills the serial extract)
topic: viewer
status: done
origin: GPU-avatar design (2026-08-12), context/gpu-avatars.md §2, §7 Phase 1
refs: [viewer-perf-gpu-avatar-crowd, viewer-perf-avatar-pose-extract-skins]
blocked_by: [viewer-perf-gpu-avatar-phase0-mesh-dedup, viewer-perf-gpu-avatar-keystone-skinuniforms-spike]
---

Context: [context/gpu-avatars.md](../context/gpu-avatars.md) §1.2(c/e/f), §2
(passes C+D, §2.3 scheduling, §2.4 write-in), §7 "Phase 1". Epic:
[[viewer-perf-gpu-avatar-crowd]].

## Progress — split into 1a (done) + 1b (remaining)

**Phase 1a — GPU pose pipeline as an additive side-by-side ghost — DONE
(2026-08-13, committed).** Buffers (rest/frame/working), pass C (FK, an
operation-for-operation port of `deformed_world_matrices` — **bit-exact** in
golden tests), pass D (palettes → `SkinUniforms` via the keystone write-in),
and the `Core3d` scheduling all landed in `src/gpu_avatars/`, behind
`SL_VIEWER_GPU_AVATARS` (default off). The CPU path is **untouched**; instead of
freezing joints, each rigged avatar renders **twice** — the CPU pose in place
and a GPU-FK **ghost 2 m aside** (with a floating "GPU" label) — so the two are
directly comparable. Rigid base parts (the two eyeballs, non-skinned) get
CPU-placed rigid ghosts. Verified: golden bit-exact FK + headless GPU-vs-CPU
(4.8e-7); live on OpenSim and **aditi** (Bento-scale mesh body) — the readback
logs `GPU palette == CPU palette` (worst diff 1.5e-5) and the two copies are
pose-identical, no validation errors. The defensive `resolve_joint_map`
(unresolvable joint → root fallback, never a dropped submesh) also landed.

**Phase 1b — capture the win — DONE (2026-08-13).** The GPU in-place path is
now the **default** (startup capability check: compute + storage buffers →
GPU-FK; downlevel/WebGL → automatic CPU fallback; `SL_VIEWER_GPU_AVATARS=cpu`
forces legacy, `=ghost` the 1a side-by-side, readback via its sub-flag). In
real mode `write_joint_globals` is bypassed (skinning joints frozen); the CPU
still samples+blends and publishes the pose to pass C. A **socket mini-FK**
(`BevySkeleton::deformed_world_chain`, sharing the per-joint head with
`deformed_world_matrices` so they can't drift) CPU-places only the socket set:
worn attachment joints, the rigid eyeballs, and `mHead` (camera). Tests:
sl-client-bevy **28/28** (incl. a new bit-exact chain test over every joint),
viewer **15/15**, all clean.

**Measured (aditi tracy, 2026-08-13):** the win landed — `ExtractSchedule`
mean **7 → 2.9 ms**, `extract_skins` no longer a cost (frozen joints),
`PostUpdate` **16.5 → 13.7 ms**, transform propagation collapsed; framerate
~**12.7 → ~23 fps** (scene not a controlled A/B). Visually confirmed correct
(natural stance, eyes seated, name tag tracking).

**Note — Phase 4's default-flip pulled forward:** GPU is the default now (not a
debug opt-in), per the "no A/B, so no default-off" decision. **Accepted
regression until Phase 3:** avatar *picking* uses rest-pose collider geometry
in real mode (frozen joints), so clicks on animated avatars are imprecise —
[[viewer-perf-gpu-avatar-phase3-gpu-picking]] fixes it. **Known limits:** the
extract collapse holds for **stationary** avatars; a **moving** avatar's
propagation re-globals+re-dirties the joint tree each frame (extract cost
returns) until Phase 4 removes the joint entities. **Open bug:**
[[viewer-gpu-avatars-1b-slow-shutdown-high-rss]] (10.6 GB RSS + ~2 min
shutdown on the tracy run — leak vs tracy-buffer undecided; observing across
future captures).

Original 1b plan (for the record):

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
