---
id: viewer-perf-gpu-avatar-phase3-gpu-picking
title: GPU avatars Phase 3 — GPU ID-buffer picking (retire the CPU raycast)
topic: viewer
status: blocked
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
