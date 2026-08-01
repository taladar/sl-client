---
id: viewer-perf-pbr-shadow-cluster-rez
title: Tune main-view shadow specialization + clustered lighting during rez
topic: viewer
status: ready
origin: Tracy profiling of Aditi rezzing (2026-07-30)
refs: [viewer-perf-probe-capture-shadows, viewer-perf-probe-scheduling]
---

Context: [context/viewer.md](../context/viewer.md).

Tracy self-time over the first ~10 s of rezzing on Aditi shows the **main-view**
PBR shadow + clustered-lighting systems as a large slice of frame time:

| System | ms/frame |
| --- | --- |
| `render::light::specialize_shadows` | 1.33 |
| `cluster::gpu::prepare_clusters_for_gpu_clustering` | 0.89 |
| `render::light::queue_shadows` | 0.87 |
| `render::gpu_preprocess::prepare_preprocess_bind_groups` | 0.66 |
| `light::check_dir_light_mesh_visibility` | 0.42 |

These are bevy-internal but driven by *our* scene: how many lights cast shadows
and the clustered-forward configuration. As prims + local lights stream in
during rez, shadow specialization and cluster preparation scale up.

(The reflection-probe / environment-map bind-group cost seen in the same capture
— `prepare_generated_environment_map_bind_groups` 0.73 ms/frame and our own
`probes::copy_probe_faces` 0.40 ms/frame — is **out of scope here**; it is
covered by the existing probe tasks [[viewer-perf-probe-scheduling]],
[[viewer-perf-probe-capture-shadows]] and siblings.)

## Spike distribution — the real problem (2026-07-31, 2-min capture)

A full 2-minute Tracy capture of aditi rezzing (4511 frames) re-measured these
as **per-event durations**, not just the mean, and the mean badly under-sells
them. Unwrapping the two shadow systems over the 39–88 s rez burst:

| System | frames >50 ms | frames 20–50 ms | peak | mean |
| --- | --- | --- | --- | --- |
| `specialize_shadows` | **165** | 45 | 186 ms | 3.98 ms |
| `queue_shadows` | **112** | 76 | 186 ms | 2.65 ms |

The spikes are **recurring across the whole rez window (39–88 s), not a startup
one-off**. Both run in the Render schedule, so a bad frame spends **300–370 ms
on shadows alone** — which is what drives the aggregate `present_frames`
(412 ms) and `RenderExtractApp` (418 ms) stalls in the same capture, i.e. the
visible multi-hundred-ms hitches while the region fills in. Root cause: every
new *material × mesh* combo that streams in needs a shadow pipeline variant
**specialized** (a shader compile) — this is the shadow-view instance of the
broader pipeline-specialization stall tracked in
[[viewer-perf-pipeline-specialization-stalls]].

Caveat: `queue_shadows` cost scales with *views × casters*, so part of it may
overlap the known probe-capture-shadow work
([[viewer-perf-probe-capture-shadows]]); `specialize_shadows` is genuinely
content-driven and view-independent (the pipeline cache is shared).

Investigate / tune:

- How many SL point/spot lights cast real-time shadows at once, and whether they
  should at all (the reference viewer is far more conservative); cap shadow
  casters or disable per-prim light shadows.
- The clustered-forward config (cluster dimensions / max lights per cluster) vs.
  the number of small local lights a rezzing region produces.
- Whether `check_dir_light_mesh_visibility` can be gated / cheaper.

Measure the main-view shadow/cluster self-time before/after with a ≤10 s
`tracy-capture` capture during active rez.
