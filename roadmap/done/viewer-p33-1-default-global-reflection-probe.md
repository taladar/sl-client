---
id: viewer-p33-1
title: Default (global) reflection probe
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 33 — Reflection probes
---

Context: [context/viewer.md](../context/viewer.md).

**P33.1. Default (global) reflection probe.** Ingest reflection-probe
volumes (the `LLReflectionProbeParams` extra-param — `ExtraParams` `0x90`)
onto an `ObjectReflectionProbe` component, and drive the scene-wide
**default** probe: the reference viewer's fallback probe used where no local
probe applies. Six 90° cameras capture the scene around the viewpoint into a
cube `Image` (via six colour targets + a render-world blit, amortized as a
brief burst a few times a second), which a Bevy `GeneratedEnvironmentMapLight`
filters into the diffuse / specular maps the Phase 27 PBR materials sample.
For consistency the sky-set `GlobalAmbientLight` is dropped and the custom
terrain / water shaders sample the probe too (terrain: diffuse irradiance as
ambient; water: specular reflection). Reference: `LLReflectionMapManager` /
`RenderReflectionProbe`. NOTE: reflection / ambient **brightness calibration
is deliberately deferred** (see P33.3) — the intensity and residual-ambient
are exposed as `SL_VIEWER_PROBE_INTENSITY` / `SL_VIEWER_PROBE_AMBIENT_SCALE`
knobs, and `SL_VIEWER_PROBE_TEST_SPHERE=1` spawns a mirror ball to inspect the
captured environment.
