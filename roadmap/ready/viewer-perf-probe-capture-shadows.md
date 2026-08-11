---
id: viewer-perf-probe-capture-shadows
title: Reduce directional shadow-cascade cost for probe capture views
topic: viewer
status: ready
origin: reflection-probe performance planning round (2026-07-22), Firestorm LLReflectionMapManager survey
refs: [viewer-perf-probe-instrumentation, viewer-perf-probe-capture-content]
---

Context: [context/viewer.md](../context/viewer.md).

The dominant fixed per-face cost once
[[viewer-perf-probe-capture-content]] has shrunk the geometry: Bevy forks
the **directional shadow cascades per camera** (bevy_pbr-0.19
`render/light.rs` builds cascade views keyed on the camera entity; point
and spot shadow maps are shared across views), so every active capture
face re-renders the sun's full cascade set. The reference renders probe
faces with only 2 of its 4 sun cascades and skips spot shadows entirely.
Bevy has no per-view cascade knob — `CascadeShadowConfig` is per-*light*
— so this task is an investigation with a likely upstream component.

Investigation order:

1. A per-view cascade-count / shadow-disable override component consumed
   where `prepare_lights` / the cascade build fork per camera — the
   natural upstream feature. Fork `bevy_pbr` under `[patch.crates-io]`
   and submit upstream (same fork, plausibly the same PR series, as
   [[viewer-perf-probe-filter-on-capture]]).
2. Excluding the sun via capture-view `RenderLayers` and adding an
   unshadowed clone light on a capture-only layer — probably
   double-lights the shared world geometry; if it is a dead end,
   document why.
3. Shrinking the cascade far bounds for capture views to the probe draw
   distance (the main view runs `first_cascade_far_bound: 24.0`,
   `sky.rs`) — fewer/smaller cascades when the view sees only ~64 m.

Also decide, with the mirror test sphere and render goldens, whether
capture views want *fewer cascades* (reference behaviour, keeps contact
shadows in reflections) or *no shadows at all* (cheapest; reflections go
shadowless — may be imperceptible after roughness filtering at 128²).

Acceptance: per-face GPU time on a shadow-heavy scene approaches the
same face rendered with shadows fully off (within ~2×), measured via
[[viewer-perf-probe-instrumentation]].

## The CPU cull cost also scales with probe-view count (2026-08-11)

Measured while profiling the sun shadow-caster cull
([[viewer-perf-pbr-shadow-cluster-rez]]): the directional cull
(`check_dir_light_mesh_visibility`) runs **once per shadow view**, and each
probe capture face-camera is a view in the light's `CascadesFrusta`. On a
rezzed aditi scene with ~3 k casters the cull iterated **~127 k caster×view
pairs** — i.e. the probe cameras multiply the CPU cull work ~40×, and that
fixed floor (list rebuild/sort + `ViewVisibility` marking) is what remains
after the caster test itself is amortised.

Implication for this task's approach: **excluding the sun from probe capture
views entirely** (the capture-view `RenderLayers` / unshadowed-clone route, or
simply not giving probe cameras cascade frusta) fixes **both** the per-face GPU
shadow-render cost this task targets **and** this CPU cull floor — whereas
merely *reducing* the cascade count for probe views (2 of 4) still leaves the
cull iterating every caster for every probe view. Prefer the exclusion route
where the mirror/golden tests allow shadowless reflections.
