---
id: viewer-animesh-intermittent-render
title: Animesh intermittently fails to render (timing race, not deterministic)
topic: viewer
status: bugs
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
