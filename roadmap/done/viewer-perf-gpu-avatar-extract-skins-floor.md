---
id: viewer-perf-gpu-avatar-extract-skins-floor
title: Eliminate CPU extract_skins for GPU-posed avatars (iterate-all floor)
topic: viewer
status: done
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

## Outcome (2026-08-14): DONE — Option A (marker in a Bevy fork)

Chose the design's Option A: a marker that excludes GPU-posed skins from
`extract_skins`' iterate-every-skin per-joint gather while keeping their
`SkinUniforms` allocation. The gather could not be skipped from our crate —
`extract_skins`' allocation state (`skin_uniform_info`, which drives each
mesh's `current_skin_index`) is private, so we depend on Bevy running the
allocation half; only the per-joint half is the floor. That forced a
`bevy_pbr` source change.

Option B (drop `SkinnedMesh` / the dummy joints entirely and skin ourselves)
was rejected: it also gives up Bevy's automatic skinned-mesh **batching**,
which matters more than the extract floor at real 40–100-avatar crowd scale,
and it needs a second per-instance channel we don't have (`MeshTag` is taken
by GPU pick IDs).

**The change** (upstreamable, deliberately not yet proposed upstream):

- A **Bevy fork** — `github.com/taladar/bevy`, branch
  `sl-client-externally-posed-skin`, cut off the `v0.19.0` tag — adds one
  thing to `bevy_pbr`: an `ExternallyPosedSkin` marker component, and a
  `Without<ExternallyPosedSkin>` filter on `extract_skins`' per-frame
  joint-gather query. The allocation query (`changed_skinned_meshes`) is left
  unfiltered, so a marked skin still gets a palette slot and a valid
  `current_skin_index`. The workspace pins every `bevy_*` 0.19.0 crate to the
  fork rev (a monorepo git patch pulls sibling members from the fork, so they
  must all point there or `bevy_render` would exist twice).
- Viewer side: `GpuSkinBinding` now `#[require(ExternallyPosedSkin)]`, so
  every GPU-posed skin (avatar base parts, worn rigged meshes, animesh) — and
  only those — carries the marker automatically. Non-avatar / gallery CPU
  skins keep the normal extract path. A unit test pins the require wiring.

**Semantics unchanged, CPU-only savings:** for a GPU-posed skin the skipped
gather only ever wrote identity matrices (all palette slots bind the one inert
dummy joint) that pass D overwrites in place the same frame — so the palette
the GPU reads is bit-for-bit what it was before, the readback verdict is
unaffected, and only the wasted per-skin×per-joint iteration is gone.

Pending: the live `CROWD=100` Tracy A/B (off-screen `extract_skins` → ~0) is a
user-run measurement — the mechanism is proven but the crowd-scale number is
not yet captured here.
