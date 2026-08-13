---
id: viewer-perf-gpu-avatar-phase4-remove-scaffolding
title: GPU avatars Phase 4 — remove joint entities + CPU pose scaffolding
topic: viewer
status: done
origin: GPU-avatar design (2026-08-12), context/gpu-avatars.md §7 Phase 4
refs: [viewer-perf-gpu-avatar-crowd]
---

Context: [context/gpu-avatars.md](../context/gpu-avatars.md) §5 (leaves-the-CPU
list), §7 "Phase 4", Appendix A. Epic: [[viewer-perf-gpu-avatar-crowd]].

**Scope decision (2026-08-13, user):** drop the CPU pose path **entirely** —
no CPU skinning fallback survives this phase. This reverses the earlier
"keeping the CPU path is fine" stance: with GPU posing proven through Phases
1–3, the `cpu`/`off` selector and the whole CPU solve/joint-entity path are
deleted, not gated. Genuinely downlevel hardware (no compute) renders avatars
at **rest pose** with a one-time WARN, not via a resurrected CPU skinner.
Rationale: this whole rewrite exists because CPU posing is too slow on *our*
(capable) hardware at crowd scale; hardware too weak to run compute shaders
would be even further underwater doing ~200-joint CPU skinning per avatar, so
a CPU fallback there would be dead code serving no usefully-fast client.
`deformed_world_matrices` the *function* stays (one-shot rest-pose uses:
spawn placement, body metrics); only its per-frame calls go.

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

## Outcome (2026-08-13): DONE — GPU-only posing, avatars + animesh

Landed incrementally (each an independently-compiling, live-verified
checkpoint) rather than one pass:

1. `GpuSkinBinding { slot, canonical }` component carries each skin's canonical
   joint indices at skin-build time; GPU resolution reads it instead of mapping
   `SkinnedMesh.joints` entities. (Live: readback `==` + visual.)
2. Per-avatar **socket entities** (root children: head + reparented attachment
   nodes + rigid parts), placed by `write_socket_locals` from
   `deformed_world_chain`, seeded to a rest solve at spawn; camera/name-tag/
   ground/chest repointed off joint entities; `pose_attachment_nodes` deleted.
3. **Atomic joint removal**: one shared inert dummy joint,
   `SkinnedMesh.joints = vec![dummy; K]`; deleted the per-avatar joint spawn,
   `pose_avatar_skeletons`'s full solve, `write_joint_globals`, `PoseGate`, the
   ghost machinery + `cpu`/`ghost` mode selector, and the CPU worn-pick chain;
   `is_rigged` off the head-socket map. Downlevel (no compute) → rest pose + a
   one-time WARN, no CPU skinner.
4. **Animesh onto the GPU path** (user directive — no CPU skinner left):
   unified `PoseSlotKey { Avatar(AgentKey), Animesh(ObjectKey) }` threaded
   through the registry/feed/staging; animesh gets a `GpuSkinBinding` + dummy
   joints, feeds passes A–D from `ObjectAnimation`; deleted
   `pose_control_avatars` + `spawn_bare_skeleton`. (It had been invisible
   because avatar joint-churn accidentally kept the scene over Bevy's
   25%-dirty extract threshold, masking a `current_skin_index` staleness bug
   on static submeshes.)

**Scope decisions vs the original plan:** CPU path dropped entirely (not kept as
a downlevel fallback) — see the decision note above. The frozen-dummy-joint
*keep-the-entities* idea was rejected: pinned `bevy_pbr` 0.16.1 `extract_joints`
has a 25 %-dirty threshold that re-extracts every skin once >25 % of joints
change, so a moving crowd of frozen-but-present joints defeats the win — the
entities had to actually go.

**Measured (aditi tracy, correct RUST_LOG):** `extract_skins` **0.76 ms/frame**
mean (was the ~5–7 ms serial bottleneck) and now **flat** — independent of
avatar/joint count. The average frame is now **render-app bound**
([[viewer-perf-render-app-bound-frame]]); single-digit-FPS spikes are
asset-streaming, not avatars ([[viewer-perf-asset-streaming-frame-spikes]]).
Follow-ups filed: [[viewer-hover-tooltip-202ms-frame-spike]],
[[viewer-mouselook-own-head-visible-from-inside]]. The tracy shutdown-hang was
diagnosed to the tracy-client worker (profiling-only, closed).
