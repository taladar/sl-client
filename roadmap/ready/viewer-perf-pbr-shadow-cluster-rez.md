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

## `check_dir_light_mesh_visibility` is a *steady-state* cost, not just rez

The 2026-07-30 table above measured `check_dir_light_mesh_visibility` at
0.42 ms/frame during rez. The full-session aditi capture (2026-08-10)
re-measures it in the **visible steady state** (`t ≥ 140 s`) at **6.5 ms
mean + 1.7 ms for its command flush = 8.2 ms/frame** — ~15× higher and
now one of the two dominant `PostUpdate` clusters
([[viewer-perf-steady-state-46fps-ceiling]]). Unlike `specialize_shadows`/
`queue_shadows` (which are on the Render thread, which currently has
headroom), this one is on the **main-thread-bound** side, so it directly
gates the frame. It is the sun casting shadows over **all** meshes,
re-tested every frame against the cascades — the single most valuable
shadow lever to make cheaper or gate. Item 3 below is now the priority,
not just a nice-to-have.

### Confirmed the top real serial PostUpdate cost (2026-08-10)

A `-u` critical-path pass (correcting the summed-self-time numbers in
[[viewer-perf-steady-state-46fps-ceiling]]) confirms this is the genuine
top single-threaded PostUpdate cost: it is **one serial system at ~5–6 ms**
wall-clock (up to ~12 ms), one zone per frame on a worker, sitting **near
the PostUpdate tail** (runs right after `check_visibility`, before the ~2 ms
close-out). Unlike the material-spec family (which parallelises to ~2 ms,
downgraded), nothing hides this cost — cutting it directly shortens the
critical chain. **This is the #1 average-frame target.**

### Plan of record: budgeted round-robin (exploit slow sun/cascade drift)

Key property: the system re-tests **every** mesh against the directional
light's cascade frustums every frame, but those frustums only shift
**slightly** frame-to-frame (the sun angle changes a little, the camera
drifts), so almost no mesh's shadow-visibility actually flips. So a full
per-frame retest is nearly all redundant.

- **Round-robin the retest (the elegant lever):** re-test only 1/N of the
  casters each frame, cycling so every caster is re-tested every N frames
  (same shape as the LOD-apply budget). A caster's shadow-visibility is then
  up to N frames stale, which is invisible at slow sun/camera speeds. Must
  still test **newly-spawned / moved** meshes immediately (change-detection),
  and ideally re-test all when the camera turns fast (cascades shift a lot).
  **Feasibility caveat:** `check_dir_light_mesh_visibility` lives in
  **`bevy_light`**, so round-robining it means replacing the Bevy system
  with our own (a `[patch.crates-io]` fork, the
  `sl-client-fork-upstream-for-upstream-bugs` path — and upstreamable).
- **No-patch levers (reduce N):** cap / distance-cull shadow casters
  (`NotShadowCaster` on distant or small meshes — the reference viewer is
  far more conservative about what casts), shorten the shadow distance, and
  reduce cascade count. These cut the mesh count tested without touching
  Bevy, and compose with the round-robin.

Expect a follow-up session after the [[viewer-r26]] mesh errors are fixed.

Investigate / tune:

- **`check_dir_light_mesh_visibility` gating/cheapening — now the priority
  (8.2 ms/frame steady, main thread).** Whether the sun-shadow caster set
  can be spatially culled, throttled, or gated on scene change instead of
  a full per-frame retest.
- How many SL point/spot lights cast real-time shadows at once, and whether they
  should at all (the reference viewer is far more conservative); cap shadow
  casters or disable per-prim light shadows.
- The clustered-forward config (cluster dimensions / max lights per cluster) vs.
  the number of small local lights a rezzing region produces.

Measure the main-view shadow/cluster self-time before/after with a ≤10 s
`tracy-capture` capture during active rez.
