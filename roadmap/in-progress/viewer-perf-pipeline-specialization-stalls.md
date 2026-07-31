---
id: viewer-perf-pipeline-specialization-stalls
title: Reflection-probe capture drives periodic shadow re-specialization stalls
topic: viewer
status: in-progress
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

Still to do: live-verify on OpenSim/aditi (eyeball reflections + a fresh Tracy
capture confirming the periodic shadow spikes are gone), then move to `done/`.
