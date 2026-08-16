---
id: viewer-perf-shadow-cull-change-driven
title: Incremental shadow-caster extraction (O(changed), not O(all) per frame)
topic: viewer
status: done
origin: shadow-cull amortisation session (2026-08-11)
refs:
  - viewer-perf-pbr-shadow-cluster-rez
  - viewer-perf-probe-capture-shadows
  - viewer-perf-frame-churn-cleanups
---

Context: [context/viewer.md](../context/viewer.md).

Follow-up to the async off-thread cull in
[[viewer-perf-pbr-shadow-cluster-rez]]. That moved the frustum tests off the
critical path, but `dispatch_shadow_cull` still
**snapshotted every caster (~40 k on a dense aditi scene) out of the ECS every frame**
— the ~1.84 ms/frame that remained on the critical path.

Key insight (do not re-litigate): we only ever stale the frustum-culling
*decision*, never the shadow — the shadow map is re-rendered from casters' live
transforms every frame. A caster with a stale include-list still casts a correct
shadow at its current position; the only artifact is a brief missing/extra
contribution at a cascade boundary.

## What was tried and abandoned: an all-or-nothing skip

First attempt: gate the whole dispatch on "did any cull input change" (sun /
camera moved, any caster changed) and skip the snapshot + pass entirely when
nothing did. **It never skipped in normal use.** Measured on OpenSim: even fully
idle and parked, `applied == fps` every second. The own avatar's idle animation
churns its casters (and the follow-camera tracking it), so the scene is never
static. Guarding the camera write (see below) did not help — the avatar alone
keeps it dirty. All-or-nothing is only useful on an avatar-free static scene,
which normal use never is.

## What shipped: incremental extraction

Instead of skipping, **keep a persistent caster snapshot and update it
incrementally.** Each frame `dispatch_shadow_cull`:

- folds only the casters that changed / spawned (`Changed<GlobalTransform>` /
  `Changed<Aabb>` / `Changed<RenderLayers>` / `Added<Mesh3d>` /
  `Changed<InheritedVisibility>`) or despawned (`RemovedComponents<Mesh3d>`)
  into a persistent `Vec<CasterInput>` + an `EntityHashMap<usize>` index (O(1)
  swap-remove / upsert) — **O(changed), not O(all)**;
- shares the snapshot with the off-thread pass via `Arc` (a refcount bump, no
  copy); `Arc::make_mut` mutates it in place because `apply_shadow_cull`
  (chained before dispatch) drops the finished task's `Arc` first — only a rare
  pass overrun forces a copy-on-write.

This is avatar-independent: only the handful of avatar casters that actually
moved are re-extracted; the ~40 k static buildings/trees stay put.

Verified on aditi (Tracy, ~36 k casters): `dispatch_shadow_cull` mean
**1.84 ms → 0.377 ms** (~5×); total shadow critical-path work `apply` + `mark` +
`dispatch` ~2.84 ms → **~1.22 ms**. Shadows track the avatar correctly. The
residual `dispatch` cost is now mostly per-view frusta gathering (the probe-view
multiplication — [[viewer-perf-probe-capture-shadows]]), and `mark` (the
`ViewVisibility` marking) is the largest remaining piece, inherent to Bevy
resetting `ViewVisibility` each frame.

Bonus, kept from the abandoned attempt: `apply_pose` now leaves the camera
transform untouched once the smoothed pose is within a sub-perceptible epsilon,
so a settled camera stops re-writing (and `Changed`-marking) its transform every
frame — a general frame-churn reduction ([[viewer-perf-frame-churn-cleanups]]),
verified visually smooth.

## Follow-up 2026-08-16 — dropped the off-thread sorts

A `perf` sample of the off-thread pass (dense aditi region) showed its two
`Vec<Entity>` `sort_unstable`s — the per-cascade `cascade.entities.sort` (the
near cascade holds ~every caster) and the `visible` `sort` + `dedup` — at
~6.9 % of total CPU, roughly **half** of `run_shadow_cull`. Neither result needs
ordering: Bevy's own `check_dir_light_mesh_visibility` pushes cascade contents
and its visible set unordered too, and the shadow phase re-sorts / batches
downstream. Removed both — visibility is now tracked in a bool vec parallel to
the caster snapshot (each caster yielded at most once, so
`mark_shadow_caster_visibility`'s `iter_many_mut` still sees a unique set
without a dedup), and the visible set is collected in one O(casters) scan.
Behaviour-preserving (unit tests unchanged); a pure off-thread CPU/power win, so
it does not move frame time (the pass is off the gating leg). The residual
`run_shadow_cull` cost is the tight per-caster `intersects_obb` loop; a
BVH-accelerated cull (parry's dynamic BVH, like the static raycast index) was
considered and deferred — sun cascades cover most of the visible scene, so a BVH
prunes mainly far casters, a partial win for a non-gating path.
