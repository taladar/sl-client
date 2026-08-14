---
id: viewer-perf-gpu-avatar-extract-skins-floor
title: Eliminate CPU extract_skins for GPU-posed avatars (iterate-all floor)
topic: viewer
status: ready
origin: GPU-avatar Phase 5 culling analysis (2026-08-14)
refs: [viewer-perf-gpu-avatar-crowd-cpu-bound, viewer-perf-gpu-avatar-phase4-remove-scaffolding]
---

Context: [context/gpu-avatars.md](../context/gpu-avatars.md).

Bevy 0.19 `bevy_pbr::render::skin::extract_skins` has a **non-cullable floor**:
its skin loop iterates **every** `SkinnedMesh` entity with no `ViewVisibility`
gate, so at `SL_VIEWER_CROWD=100` (~500 skinned submeshes) `extract_skins` never
drops below ~2.16 ms even with the whole crowd off-screen (only the expensive
per-visible-skin joint extraction is visibility-gated). This surfaced while
verifying the GPU-bounds culling: `render_system` dropped ~20 ms off-screen but
`extract_skins` stayed flat — the floor, not a culling failure.

For GPU-posed avatars this CPU work is **wasted**: pass D overwrites the palette
in `SkinUniforms`, and the skins bind the single shared dummy joint, so the CPU
extraction produces nothing the GPU uses. It exists only to keep Bevy's skin
allocator/`current_skin_index` plumbing alive (the Phase 4 keystone).

## Direction

Stop Bevy's `extract_skins` from doing per-frame work for GPU-avatar skins
without losing the `SkinUniforms` slot registration pass D writes into. Options
(pick after a spike):

- A marker that excludes GPU-avatar skins from `extract_skins`'s iterate-all
  loop while keeping their `current_skin_index` allocation (needs a Bevy-side
  hook, or a fork — see the fork-upstream-for-upstream-bugs memory).
- Maintain our own palette buffer + a forked `skinning.wgsl` binding for GPU
  avatars (the §2.4 last-resort, decouples us from `extract_skins` entirely).
- Upstream an "externally-written skin" marker to Bevy (the §7 endgame).

Removing the floor makes the frustum culling's savings show up in
`extract_skins` too and cuts the crowd Extract cost
([[viewer-perf-gpu-avatar-crowd-cpu-bound]]).

## Verify

`CROWD=100`: `extract_skins` drops toward ~0 when the crowd is off-screen (the
floor gone), palettes still correct (readback `==`), avatars still render.
