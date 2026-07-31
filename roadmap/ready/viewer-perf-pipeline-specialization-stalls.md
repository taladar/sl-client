---
id: viewer-perf-pipeline-specialization-stalls
title: Pre-warm / cache render-pipeline specialization to stop first-use compile stalls
topic: viewer
status: ready
origin: Tracy profiling of Aditi rezzing (2026-07-31, 2-min capture)
refs: [viewer-perf-pbr-shadow-cluster-rez, viewer-perf-slfaceext-material-reprep]
---

Context: [context/viewer.md](../context/viewer.md).

A full 2-minute Tracy capture of aditi rezzing (4511 frames) shows several Bevy
pipeline-specialization systems as sharp single-frame outliers — the first time
a given pipeline variant is needed, wgpu compiles the shader on the frame
thread and the frame stalls. Self-time, per-event:

| System | peak | mean |
| --- | --- | --- |
| `upscaling::prepare_view_upscaling_pipelines` | 121 ms | 0.03 ms |
| `material::write_material_bind_group_buffers` | 112 ms | 1.66 ms |
| `material::queue_material_meshes` | 47 ms | 0.34 ms |
| `material::specialize_material_meshes` | 22 ms | 0.30 ms |

`prepare_view_upscaling_pipelines` is a pure startup one-off (84 % of its whole
2-minute total is one call); the `material::*` systems recur during material
churn as content streams.

The single worst instance of this whole class is the shadow-view specialization
already tracked in [[viewer-perf-pbr-shadow-cluster-rez]] (`specialize_shadows`
165 frames >50 ms) — same root cause (a new *material × mesh* combo forces a new
pipeline variant), just on the shadow view. This task is the **general**
mitigation that applies to all views.

The reference viewer avoids these stalls by pre-compiling / warming the shader
cache. Investigate:

- Whether Bevy's pipeline cache can be **pre-warmed** for the small, known set
  of pipeline variants the viewer actually uses (opaque / alpha-blend / alpha-
  mask × skinned / static × shadow / no-shadow), so the first prim of each kind
  does not pay a compile stall mid-rez.
- Whether the number of *distinct* material/mesh specialization keys can be
  **reduced** (fewer `StandardMaterial` flag permutations, fewer vertex-layout
  variants) so there are fewer variants to compile at all.
- `prepare_view_upscaling_pipelines` is a pure one-off at startup — cheap to
  pre-warm during the login/loading screen rather than on the first rendered
  frame.

Measure the specialization systems' per-event max (not just mean) before/after
with a multi-minute capture during active rez — the mean hides these; only the
per-event spike distribution shows them (see
`book/src/tools/profiling.md` for the capture/export recipe).
