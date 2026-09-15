---
id: viewer-animesh-intermittent-render
title: Animesh intermittently fails to render (timing race, not deterministic)
topic: viewer
status: done
origin: Aditi Tracy captures + a logged diagnostic run (2026-08-14)
refs: [viewer-profiling]
---

Context: [context/viewer.md](../context/viewer.md).

An in-world **animesh** (animated object) sometimes does not render on Aditi.
Reported as regressed since around the end of the GPU-avatar refactor (Phase
4/5), **not** the later `ui`/`test` branch merges (those touched no code
adjacent to the animesh/GPU-pose path — one had no conflicts, the other only
import conflicts).

## Observations (2026-08-14)

- **Intermittent, not deterministic.** Two full-session Tracy captures on the
  same Aditi region both showed the animesh missing; a third run (same binary,
  **no** Tracy capture, with the diagnostics below) rendered it correctly. The
  binary in all three carried the uncommitted asset-upload budgeting work, but
  the issue predates it.
- **When it renders, everything is correct.** The Phase 5 bounds census
  (`SL_VIEWER_LOG_AVATAR_BOUNDS=1`) resolved cleanly on the good run: after a
  brief rez transient (44 submeshes on the small/default AABB) it reached
  **696/696 real AABBs** (half-extents ~2.0–4.4 m, every slot resolved, 690
  ViewVisible). So the failure is **not** a deterministic frustum-cull
  (degenerate/mislocated posed `Aabb`) nor a collapsed GPU pose — both
  hypotheses were disproven by the healthy run.
- **Correlates (weakly, n small) with Tracy load** → a **timing race**, most
  likely the `apply_rigged_attachments` `finest-LOD upgrade in flight` gate
  (`objects.rs`): an animesh binds only once its finest LOD block decodes and
  applies (via the budgeted `apply_object_meshes`); under Tracy's per-system
  span overhead the frames are slower and that race can lose, leaving the
  animesh permanently `not yet bound`.

## Not yet ruled out

Whether the shared mesh-upload budget
([[viewer-perf-asset-streaming-frame-spikes]] / the `MeshUploadBudget`
unification) contends the finest-LOD `apply_object_meshes` under Tracy load and
worsens the race. F3 looked drained on the failing run, but an in-flight
finest-LOD upgrade may not be counted there.

## Decisive next experiment

One run reproduces the failing conditions **and** captures the failure mode:
a **Tracy capture** with `SL_VIEWER_LOG_ATTACHMENT_BIND=1
SL_VIEWER_LOG_AVATAR_BOUNDS=1` and `sl_client_bevy_viewer=info`, driven to the
same animesh.

- If the census at failure shows the animesh's submeshes stuck on the
  **default/small** AABB with the bind log repeating `finest-LOD upgrade in
  flight` → confirm the finest-LOD bind race; fix by not gating the animesh
  bind on the finest LOD (bind the coarse block, swap on finest) or by
  prioritising the finest-LOD apply.
- Re-run with `SL_VIEWER_MESH_UPLOAD_BUDGET=999999` (budget disabled): if it
  then renders under Tracy, the budget is implicated and rigged-bind / its
  finest-LOD apply needs a reserved slice ahead of LOD churn.

Diagnostics already in-tree: the Phase 5 census
(`gpu_avatars::stage::log_avatar_bounds`) and the attachment-bind skip log
(`SL_VIEWER_LOG_ATTACHMENT_BIND`, `objects::apply_rigged_attachments`).

## Closed as not reproducible (2026-09-15)

Two aditi runs under a full-session Tracy capture — the condition the report
tied the failure to — both rendered the animesh correctly, with
`SL_VIEWER_LOG_ATTACHMENT_BIND=1 SL_VIEWER_LOG_AVATAR_BOUNDS=1` and the new
per-animesh census below on.

The second run's census covers **15 animesh** in view (2 to 52 mesh parts
each). The last block logged for every one of them has every part **built**:
decoded at the finest level its header offers, skin decoded, no LOD change in
flight, every face posed on that animesh's own pose slot, carrying a read-back
bound, not hidden, and in view. A handful of parts passed through `waiting on
skinned bind` once on the way and moved on; none stayed there, and the
`finest-LOD upgrade in flight` gate this task suspected never held a part.

The report rested on three runs on 2026-08-14 (two failing under Tracy, one
good without). Much of the path has changed since: the failed-asset retry
budget (`6e6e0aa4`), the world-scoped store purge (`58376e06`), the rig joint
index bound (`2a310617`), the world/avatar crate split (`8a8dac95`,
`58ec4983`), and the shared mesh-upload budget (`ab949f75`) landed the same
day the report was filed. None of them is identifiable as *the* fix, so this
is closed as not reproducible rather than fixed.

### If it comes back

Run with `SL_VIEWER_LOG_ANIMESH=1` (and `RUST_LOG` at `info` for
`sl_viewer_world_avatar`). `gpu_avatars::stage::log_animesh_census` logs, for
each animesh whose census changed, one line per linkset mesh part naming its
stage — `waiting on mesh decode`, `waiting on skinned bind`, or `built` — with
the decoded vs finest-available level, whether the skin decoded, whether a LOD
change is in flight, and for a built part how many faces are posed on the
animesh, bounded, not hidden, and in view, plus whether the control avatar is
spawned and its pose slot allocated. The playing-animation count is reported
but not compared, so a scripted animesh cycling its motions does not re-log
every second. The last block logged for the missing animesh names the stage it
is stuck in; a part whose linkset root never arrived resolves to no animesh and
shows up in the `SL_VIEWER_LOG_ATTACHMENT_BIND` trace instead.
