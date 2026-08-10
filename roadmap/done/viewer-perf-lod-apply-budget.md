---
id: viewer-perf-lod-apply-budget
title: Budget prim/tree LOD re-tessellation application across frames
topic: viewer
status: done
origin: Tracy full-session aditi capture (2026-08-10)
refs:
  - viewer-perf-steady-state-46fps-ceiling
  - viewer-perf-decoded-geometry-budget
  - viewer-perf-prim-texture-apply-burst
---

Context: [context/viewer.md](../context/viewer.md).

The 2026-08-10 aditi capture measured the biggest single frame spike as
`apply_prim_lod`'s command flush — up to **358 ms** in one frame (@231 s),
firing alongside `allocate_and_free_meshes` (51 ms) and
`collect_meshes_for_gpu_building` (42 ms) — see
[[viewer-perf-steady-state-46fps-ceiling]].

Root cause: `apply_prim_lod` (and its tree twin `apply_tree_lod`)
`drain()` their **entire** target set in one frame, each entry doing a
`despawn_prim_faces` + `spawn_cached_prim_faces` (tessellate-if-uncached +
spawn face entities + queued commands). The upstream `drive_render_priority`
is throttled to 4 Hz (`REPRIORITIZE_INTERVAL_SECS = 0.25`) and, on each
tick, **clears and fully repopulates** the target maps for every visible
prim/tree. So on the one frame per 0.25 s tick that drains the fresh batch,
a camera move that re-tiers many prims at once dumps all of their
re-tessellations into a single frame.

The budget infrastructure already exists for the sibling paths —
`SpawnBudget` (object spawn), `GeometryApplyBudget`
([[viewer-perf-decoded-geometry-budget]], decode-result mesh/sculpt/rigged
apply), `TextureApplyBudget` ([[viewer-perf-prim-texture-apply-burst]]) —
but the **LOD re-tessellation apply** was never wired into any of them.

## Fix

A new `LodApplyBudget` resource (default 8 builds/frame, env
`SL_VIEWER_LOD_APPLY_BUDGET`), refilled each frame by
`reset_lod_apply_budget` and **shared** by `apply_prim_lod` and
`apply_tree_lod` (chained reset → prim → tree, mirroring
`reset_geometry_apply_budget` → mesh → sculpt). Kept **separate** from
`GeometryApplyBudget` for the same reason that one is separate from
`SpawnBudget`: a login decode burst must not starve LOD catch-up, and a
camera-sweep LOD burst must not starve decode application (the two bursts
rarely coincide anyway — decode is login, LOD is movement).

Because the target maps are `HashMap`s (not FIFO queues) and
`drive_render_priority` re-derives them wholesale every tick, the applier
uses a **budgeted retain** instead of a `drain()`:

- resolved/irrelevant entries (object gone, not a plain prim, already at
  the desired LOD) are dropped **free** (no budget charged) — the same
  "cheap items are free" rule as `drain_budgeted`;
- up to `budget` genuine re-tessellations run and are dropped;
- the over-budget remainder is **kept in the map** for the next frame.

Leaving a remainder is safe: between ticks `drive_render_priority`
early-returns (does not clear), so the applier keeps chipping the backlog
down each frame; on the next tick the map is cleared and repopulated, and
any prim still not at its desired LOD is simply re-added (idempotent — its
actual LOD has not changed). Worst case, a fast sweep's LOD catch-up lags a
few frames behind instead of freezing the frame.

The retain/budget arithmetic is factored into a testable
`retain_lod_budgeted` helper (unit-tested like `drain_budgeted`); the
per-prim/per-tree rebuild stays in the systems.

### Region departure (`DisableSimulator`) leaves no stale targets

On `DisableSimulator`, `sl-proto`'s `forget_sim_objects` emits a per-object
`ObjectRemoved` for the retiring region, so `update_objects` drops each
from `ObjectState` and despawns its entity. A LOD target left un-applied
for such an object is not misapplied: the budgeted retain sees
`state.objects.get_mut(&scoped) == None → LodOutcome::Resolved` and drops
it **free every frame, regardless of budget** (retain visits every entry;
only the rebuild-vs-defer choice is budget-gated). No eager per-circuit
purge of the target maps is added: `drive_render_priority` already
`clear()`s both maps every 0.25 s tick and repopulates only from
currently-visible objects, so a departed region's whole target set is
bulk-dropped by that clear within one tick — an eager
`retain(|k, _| k.circuit != departed)` would only duplicate it for the
bounded handful of visible prim/tree targets that linger in the frames
between the removal and the next clear.

## Verified (2026-08-10)

- Unit tests on `retain_lod_budgeted`
  (`lod_budget_charges_rebuilds_only_and_keeps_overflow`,
  `lod_budget_applies_all_when_under_budget`): at most `budget` builds,
  resolved entries dropped free, over-budget entries kept.
- Live A/B on aditi (same tracy build, comparable camera sweeps, only
  `SL_VIEWER_LOD_APPLY_BUDGET` differs). The `apply_prim_lod` command
  flush — the spike source:

  | | unbudgeted (`=1000000`) | budgeted (`=8`) |
  | --- | --- | --- |
  | max spike | **47.6 ms** (+ repeated 35–47 ms) | **2.9 ms** |
  | mean | 1.81 ms | 0.61 ms |

  ~16× off the worst frame, ~3× off the mean, and the 35–47 ms spike
  cluster is gone; rebuilds still run every frame (commands n≈3638, mean
  0.6 ms) so LOD keeps applying, just spread out — no starvation. The
  original full-session capture's 358 ms one-frame batch is exactly the
  dump this prevents.
