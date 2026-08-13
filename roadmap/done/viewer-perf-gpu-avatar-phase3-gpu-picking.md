---
id: viewer-perf-gpu-avatar-phase3-gpu-picking
title: GPU avatars Phase 3 — GPU ID-buffer picking (retire the CPU raycast)
topic: viewer
status: done
origin: GPU-avatar design (2026-08-12), context/gpu-avatars.md §6, §7 Phase 3
refs: [viewer-perf-gpu-avatar-crowd, viewer-perf-hover-pick-raycast, viewer-avatar-mesh-accurate-pick]
blocked_by: [viewer-perf-gpu-avatar-phase0-mesh-dedup]
---

Context: [context/gpu-avatars.md](../context/gpu-avatars.md) §6, §7 "Phase 3".
Epic: [[viewer-perf-gpu-avatar-crowd]]. Parallelizable with Phase 2 once
Phase 0 lands (needs the pick-ID `MeshTag` + `PickRegistry` scaffolding, which
Phase 0 seeds).

A cursor-cropped **9×9 ID + depth pick view** reusing `skinning.wgsl` (so a
GPU-posed avatar is picked exactly where it draws — morph/pose included),
async `Readback` (1–2 frame latency), IDs on `MeshTag`
(`class:4 | index:28`). Consumers: hover tooltip (deletes the
[[viewer-perf-hover-pick-raycast]] `MeshRayCast` cost), click/right-click
select, land pick + distance via depth unproject, optional box-select. Replaces
`avatar_pick.rs` CPU skinning + `fit_avatar_pick_colliders` + the world
`MeshRayCast` casts in hover/object/land picking (keeps `MeshRayCast` for
non-cursor consumers like edit-tool axis rays). Supersedes the CPU-skin/readback
options weighed in [[viewer-avatar-mesh-accurate-pick]] and fixes its
centimetre error.

Verify: pick-parity suite over a grid of cursor points (posed avatar +
overlapping prim + terrain) against the CPU answers before deleting them;
`update_hover_tooltip` cost → ~0; click-select latency feel-check. Risk: Bevy
0.19 custom-phase friction — spike the static-mesh pick pipeline on prims first
(§9.1 risk 2).

## Outcome (2026-08-13): DONE

Landed: `gpu_pick.rs` + `gpu_pick/{render}.rs` + `pick.wgsl` (9×9 crop ID +
depth, `skinning.wgsl` reuse, async `Readback`, `PickRegistry` slot tags),
`avatar_pick.rs` (845 lines) + `fit_avatar_pick_colliders` deleted, all
cursor consumers (hover / click / right-click / object / land / double-click
teleport / inventory-drag) rewired to the GPU pick. Plus the **pipeline
pre-warm** (Tier 1 of [[viewer-perf-pipeline-prewarm-during-login]]):
`collect_pick_warm_set` → `warm_gpu_pick_pipelines` specializes both pick
variants (same `GpuPickKey` + real `mesh.layout`) as pickables rez, so the
first pick after login no longer misses a still-compiling pipeline.

Verified live on aditi (release `--features profile-tracy`): hover, left /
right click, land pick, and **mesh-accurate pick on a dancing avatar** all
correct.

Two bugs surfaced by that live pass and fixed in the same change:

- **Skinned parts were never cursor-crop-culled** — every avatar's skinned
  parts across the whole region were unconditional pick candidates, so a
  crowd filled `PICK_ITEM_CAP` (512) with off-cursor parts and the loop
  `break`'d, silently dropping the part under the cursor in arbitrary ECS
  order (symptom: a hanging arm would not pick; 396 `candidate cap 512
  reached` warns in one 3-min session). Fixed by crop-culling skinned parts
  too, bounded by a fixed 3 m box at the avatar origin (every part is
  `ChildOf(root)`). Re-verified: **0** cap warns in the follow-up session.
- **Center-mass click greyed out Profile** — a hit on a worn mesh-body submesh
  routes to the attachment pie (as designed), whose Profile slot was still
  stubbed `when: Some(UNIMPLEMENTED)` even though the shared handler already
  dispatches it to the wearer. Fixed: Profile → `when: None` on both
  attachment pies (self + other); the pinning test updated.
