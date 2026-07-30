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

Investigate / tune:

- How many SL point/spot lights cast real-time shadows at once, and whether they
  should at all (the reference viewer is far more conservative); cap shadow
  casters or disable per-prim light shadows.
- The clustered-forward config (cluster dimensions / max lights per cluster) vs.
  the number of small local lights a rezzing region produces.
- Whether `check_dir_light_mesh_visibility` can be gated / cheaper.

Measure the main-view shadow/cluster self-time before/after with a ≤10 s
`tracy-grab.sh` capture during active rez.
