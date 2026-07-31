---
id: viewer-perf-pipeline-specialization-stalls
title: Reflection-probe capture drives periodic shadow re-specialization stalls
topic: viewer
status: done
origin: Tracy profiling of Aditi rezzing (2026-07-31, 2-min capture)
refs:
  - viewer-perf-pbr-shadow-cluster-rez
  - viewer-perf-slfaceext-material-reprep
---

Context: [context/viewer.md](../context/viewer.md).

## Root cause (confirmed 2026-07-31 from the 2-min Tracy capture)

The original framing — "first-use shader compile stalls the frame thread, so
pre-warm the pipeline cache" — is **wrong for the recurring spikes**. Findings:

- **Pipeline compilation is async on this build.** `avian3d`'s `parallel`
  feature pulls in `bevy/multi_threaded`, so Bevy compiles pipelines on the
  `AsyncComputeTaskPool` (`create_pipeline_task`); `synchronous_pipeline_
  compilation` is at its default `false`. wgpu compiles do **not** block the
  frame thread. Bevy 0.19 also has no wgpu disk pipeline cache
  (`create_pipeline_cache` does not exist), so cross-run persistence is moot.
- **The `specialize_shadows` / `queue_shadows` spikes are periodic — exactly
  every 90 frames**, holding rock-steady while the wall-clock period only drifts
  (5.3 s → 3.3 s) as fps rises when rez subsides. Not a startup one-off, not
  entity-influx.
- **Driver: the reflection-probe capture.** `probes.rs` re-captures a rig's six
  cube faces one-per-frame (a 6-frame burst) then idles for the rest of
  `CAPTURE_PERIOD_FRAMES`. Each capture camera is an ordinary `Camera3d`, so
  Bevy builds **4 sun shadow cascades per capture camera**
  (`build_directional_light_cascades` filters only on `is_active`). When a
  capture camera goes inactive between cycles, `specialize_shadows` **purges its
  per-view pipeline cache** (`retain(|view| all_shadow_views.contains(view))`),
  so each 90-frame re-capture re-specializes **every shadow-caster × 4
  cascades** from scratch → the 100–186 ms burst, clustered over the ~6 capture
  frames.
- Distribution (specialize_shadows): 165 frames >50 ms, peak 186 ms, mean
  3.98 ms; queue_shadows 112 frames >50 ms. A bad frame spends 300–370 ms on
  shadows → the visible periodic hitch.

So this is a **smoothness** problem (a hitch every few seconds), not
average-fps, and the fix is not pipeline pre-warming — it is stopping probe
captures from generating sun-shadow work at all.

## Reference behaviour (Firestorm `llreflectionmapmanager.cpp`)

- Probe captures render **no sun shadow maps**: `generateSunShadow()` is called
  only in the main display path (`llviewerdisplay.cpp`), never under
  `gCubeSnapshot`.
- **Default/ambient probe renders environment only** — sky / WL-sky / water /
  terrain / clouds, **zero geometry** — and updates on a lazy
  `RenderDefaultProbeUpdatePeriod = 2 s`.
- Local probes render full geometry (avatars/alpha/particles kept by default),
  amortized one face/frame round-robin; only the single closest dynamic (mirror)
  probe is per-frame, alternating radiance/irradiance passes.

Our divergence: our capture cameras had **no render restriction** and generated
full shadow cascades — heavier and far spikier than the reference.

## Plan (locked with user 2026-07-31) — near-real-time probes are the goal

Bevy gates shadow-view creation by *light-layers ∩ camera-layers*
(`prepare_lights`) and shadow-casting by *light-layers ∩ mesh-layers*
(`check_dir_light_mesh_visibility`), and has **no per-camera shadow flag** — so
the only lever is render layers. Bevy 0.19 has no `RenderLayers`
auto-propagation but the viewer already uses `Propagate<RenderLayers>`
(HUD/gizmos).

## Implemented (2026-07-31)

Done in this pass (new `probe_layers.rs` + edits to `probes.rs`, `sky.rs`,
`objects.rs`, `avatars.rs`, `lights.rs`, `terrain.rs`, `water.rs`,
`render_scene.rs`, `render_readback.rs`, `render_gallery.rs`):

- Render-layer split (`PROBE_ENV`/`PROBE_GEOM`/`PROBE_DYNAMIC` = layers 4/5/6,
  clear of HUD=1/gizmo=3/preview=8+). World geom tagged via `Propagate` on the
  `SceneObject` root (avatar → dynamic); environment tagged directly on the
  sky / disc / cloud / star / terrain / water leaves; avatars tagged via
  `Propagate` on the `AvatarAnchor` root (a *separate* root the object's
  propagate does not reach); HUD attachments override back to the HUD layer in
  `route_hud_attachment`. Prim lights inherit the object's layers, so they
  still light the main view + local probes.
- Shadow-free **mirror sun** (`SceneSunMirror`) on the probe layers, driven
  alongside `SceneSun`; probe capture cameras render the probe layers only, so
  the shadow-casting sun builds no cascades for them → the 90-frame spike is
  removed.
- Default probe camera = env-only; local probe cameras = env + geom (+ dynamic
  per the setting).
- Reference-faithful **tiered cadence**: local probes on a continuous
  oldest-first distance-weighted round-robin (no idle); default probe gated to
  `DEFAULT_PROBE_PERIOD_SECS` (2 s). `pick_next_rig` is a pure, unit-tested
  policy.
- `render_reflection_probe_dynamic_content` persistent setting (default
  **include** during development, to measure the full faithful cost) →
  `ProbeDynamicContent`.
- The headless gallery / readback harnesses build scenes outside the real
  pipeline, so they register `HierarchyPropagatePlugin::<RenderLayers>` and
  `scene_root()` propagates the probe layers onto the whole synthetic scene
  (its meshes and its own lights). The `the_mirror_reflects_each_neighbour`
  readback test passes, exercising the full new capture path end to end.

**Deferred (follow-up, same task):**

- **Change-detection skip** (skip a probe face re-capture when nothing in its
  frustum changed) — deliberately left out so this pass measures the *faithful*
  cadence cost, not a change-detected lower bound.
- Particles / flexi not yet split onto `PROBE_DYNAMIC` (only avatars are), so
  the dynamic-content setting, when off, still keeps them in probes. World
  particles stay main-view-only for now.
- No hero/mirror-probe realtime tier (all-6-faces-per-frame for the closest
  dynamic probe) — we have no mirror-probe type yet.

## Verified — fresh Tracy capture (2026-07-31, aditi, 3:04 / 5063 frames)

A post-fix 2-min+ capture (`RUST_LOG=warn,bevy_ecs=info,bevy_render=info`, clean
close) confirms the periodic shadow spikes are **gone**:

| Zone | Before (pre-fix) | After (this capture) |
| --- | --- | --- |
| `specialize_shadows` | mean 3.98 ms, peak 186 ms, 165 frames >50 ms | mean 0.58 ms, **max 15.25 ms** |
| `queue_shadows` | 112 frames >50 ms | mean 0.89 ms, **max 19.6 ms** |

No frame anywhere near the old 300–370 ms shadow bursts, and no fixed
~90-frame periodicity remains. The >80 ms outliers that survive (131 of 5062
frames) fall in **irregular 15–40 s-apart bursts** driven by content-arrival
waves — texture GPU uploads (`GpuImage` up to 66 ms), material queue /
bind-group rebuilds when a batch of new material entities appears
(`queue_material_meshes` 60 ms, `write_material_bind_group_buffers` 60 ms), the
one-off avatar-bake apply (`apply_own_local_bake` 70 ms), and one-off text
layout (`text_system` 147 ms) — **not** shadows. The single 472 ms frame is
frame 0 (first-use pipeline warmup, e.g. `prepare_view_upscaling_pipelines`
121 ms). The steady sustained cost (median ~33 ms once rezzed, vs ~17 ms on the
near-empty region) is dominated by the material-mesh pipeline
(`queue_material_meshes` 3.6 + `specialize_material_meshes` 3.3 +
`write_material_bind_group_buffers` 1.4 ms/frame), the per-frame-per-view
frustum-visibility passes (`check_visibility` — multiplied by each active probe
capture view; the mesh *extraction* around it is already
`Changed`/`AssetChanged` gated and only bites while content streams in), and
`ui_layout_system`
(2.43 ms/frame — see [[viewer-perf-ui-layout-per-frame-relayout]]) — none of it
the shadow path. (Zone `count`s in the export are par_for_each batch spans over
all 5063 frames, ~45–118/frame = views × parallel fan-out, **not** an entity
count.)

**This is a CPU/main-thread bottleneck, not a GPU one** (confirmed by a second
capture with the render/entity/system diagnostics added, 2026-07-31). On the
RX 7900 XTX the instrumented render passes sum to only ~1.15 ms GPU/frame
(`main_opaque_pass_3d` 0.46 ms mean / 3.16 ms max, `msaa_writeback` 0.45 ms
steady, everything else <0.1 ms) against a ~30 ms frame — the GPU is ~90 % idle
and the frame is spent CPU-side in the ECS/render-prep cluster above. Measured
`entity_count` climbs ~4 k → ~27 k (60 s) → tens of thousands as the region
rezzes, and the per-frame cost tracks it. A per-kind breakdown (a third capture
with `entity/*` diagnostics) shows the population is dominated by
**tessellated prim faces — ~50 % of all entities** (13.9 k of 27.6 k, one
entity per face, 98 % of everything with a `Mesh3d`), then object roots
(~3.5 k), UI nodes (~2.5 k, constant — what `ui_layout_system` re-lays-out each
frame), and ~40 % non-rendered "other" ECS (object roots + ~7.4 k
bookkeeping/hierarchy entities); the **render world holds only ~450 entities**
(mesh data lives in GPU buffers, so the CPU cost is the main-world population,
not the render world). Each face is an ordinary `Aabb`-managed entity
(`objects.rs`), so it is frustum-culled, `Aabb`-refreshed, extracted and
queued **individually every frame per view** — that per-entity work is what
tracks the count. The lever that cuts it is reducing the number of
`Aabb`-bearing entities, i.e. collapsing *a single prim's* faces into **one
multi-material mesh entity** (culling drops from per-face ~14 k to per-object
~3.5 k — the reference viewer's model: cull per drawable, batch faces by
texture only for *drawing*). NB this is distinct from same-texture batching
*across* prims: that cuts draw calls but **not** frustum culling, since each
prim instance is spatially distinct and keeps its own `Aabb` (per-instance
culling is a floor you cannot merge past). It is also not free — per-face
material / texture-anim / media-on-a-prim / picking currently rely on separate
face entities. Alongside that: entity/draw reduction (LOD, culling), UI
node-count / relayout work, and per-frame-system gating
([[viewer-perf-run-condition-gating]],
[[viewer-perf-ui-layout-per-frame-relayout]]) — **not** shader/GPU work,
which has ample headroom here. Process
memory stayed flat (~13.7 % of RAM) over 2:43, and `net/circuits` held at 1 (a
single-region session, so no per-region normalisation was needed this run).

Traces (gitignored): `tracy-captures/aditi-probe-fix-verify.tracy` (shadow
verification) and `tracy-captures/aditi-newdiag.tracy` (new diagnostics).

**Closed 2026-07-31.** Tracy half verified (above); reflections eyeballed
in-world on aditi and nothing looked wrong — though that spot had no strongly
reflective objects to stress them visually, so the correctness assurance rests
mainly on the passing `the_mirror_reflects_each_neighbour` readback test. The
deferred follow-ups (change-detection skip, particles/flexi on `PROBE_DYNAMIC`,
a hero/mirror realtime tier) remain as future work but are out of scope for this
spike-removal task.
