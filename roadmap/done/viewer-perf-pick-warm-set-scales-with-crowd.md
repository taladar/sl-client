---
id: viewer-perf-pick-warm-set-scales-with-crowd
title: collect_pick_warm_set scales with the crowd (touches un-pickable copies)
topic: viewer
status: done
origin: GPU-avatar Phase 5 crowd measurement (2026-08-14)
refs: [viewer-perf-gpu-avatar-phase3-gpu-picking, viewer-perf-gpu-avatar-crowd-cpu-bound]
---

Context: [context/gpu-avatars.md](../context/gpu-avatars.md) §6.

`collect_pick_warm_set` (the Phase 3 pick-pipeline pre-warm) grows with the
crowd: **~0.9 ms at 3 avatars → ~3.3–4.3 ms at `SL_VIEWER_CROWD=100`**. The
synthetic crowd copies are deliberately **not pickable** (`PoseSlotKey::Crowd`
is kept out of the pick registry / carries no `PickId`), so the warm set should
**not** grow with them — the pre-warm only needs one entry per distinct
`(skinned, mesh-layout)`, which the same-body crowd all shares.

## Likely cause

`collect_pick_warm_set` probably iterates all pickable-ish meshes (or all
`Mesh3d` + `Has<SkinnedMesh>`) and pushes a warm entry per entity instead of
deduping by mesh layout, so 100 copies × ~45 submeshes inflate it. It only needs
the **distinct layouts**, already warmed by the first instance.

## Fix

Dedup the warm set by `(skinned, mesh layout id)` (or restrict its query to
actually-pickable entities), so it's O(distinct layouts), not O(pickable
instances). The pipelines are cache-keyed by layout anyway, so re-collecting per
instance is pure waste. Contributes to the crowd Main cost
([[viewer-perf-gpu-avatar-crowd-cpu-bound]]).

## 2026-08-15 aditi capture — it also scales with prim-face count, every frame

On the full-session aditi trace (`tracy-captures/aditi-2026-08-15.tracy`,
dense region, **only the primary avatar** — no synthetic crowd),
`collect_pick_warm_set` is **3.77 ms/frame** — the single largest steady
`Update` system. The dedup-by-`(skinned, mesh_id)` fix from this bug is already
in code (`gpu_pick.rs:546-553`), so the *output* set is small; the cost is that
it still **re-iterates every `With<PickId>` entity every frame**, and on a dense
region that is all ~14 k tessellated prim faces (each face is pickable). So the
system is O(pickable entities)/frame regardless of crowd — the warm set barely
changes frame to frame, yet it is fully rebuilt each frame.

The deeper fix is to make it **change-driven**: rebuild the warm set only when a
new pickable `(skinned, layout)` appears (a `PickId`/mesh-layout arrival),
carrying the deduped set across frames, instead of a per-frame full scan of
every pick-tagged entity. That fixes both the crowd case and this prim-face
case. (If [[viewer-perf-custom-static-raycast-index]] later absorbs pick
raycasts, revisit whether the render-world pick pre-warm is still needed at
all.)

## Fixed (2026-08-16) — made change-driven

`collect_pick_warm_set` no longer scans every `With<PickId>` entity per frame.
Its query is now filtered to `Or<(Added<PickId>, Changed<Mesh3d>)>` — a mesh is
emitted only the frame it becomes pickable or its handle swaps (placeholder →
decoded), so in steady state it matches nothing and costs ~0 (down from ~3.6
ms/frame on a dense region). The render world (`warm_gpu_pick_pipelines`) folds
each emitted candidate into a persistent `PendingPickWarm` retry set and warms
it as soon as its mesh uploads, dropping it once specialized; a not-yet-uploaded
mesh is retried there instead of being re-collected main-side.

Warming stays tied to **rez**, never deferred to the first pick: `Added<PickId>`
fires at rez, and the render-side retry guarantees the pipeline is queued as
soon as the mesh is on the GPU — well before any pick reaches it. The crowd case
is covered for free (synthetic `Crowd` copies are never pick-tagged, so they
never enter the query). Unit-tested
(`warm_set_is_change_driven_not_a_full_rescan`): a newly-tagged mesh is emitted,
an unchanged set collects nothing, a mesh-handle swap re-emits.
`clippy --all-targets` clean.

**Measured (2026-08-16 re-capture,
`tracy-captures/aditi-2026-08-16-postpickwarm.tracy`):** `collect_pick_warm_set`
mean **3.64 → 0.166 ms/frame**, max 25.2 → 1.47 ms (~22×; the 0.166 residual is
the rez-heavy early frames where `Added<PickId>` legitimately fires — ~0 once
the scene settles). `Main` schedule total dropped **29.2 → 25.7 ms** in step.
Main-thread win only — the ~53 ms median is gated by the render leg
([[viewer-perf-render-app-bound-frame]]).

## Original report / Verify

`CROWD=100`: `collect_pick_warm_set` stays flat (~its 1-avatar cost) instead of
scaling with copy count; first pick after login still lands (pre-warm intact).
On a dense-prim region with no crowd it should also stay flat frame-to-frame
(near-0 in steady state), not track the pickable-entity count.
