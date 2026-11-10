---
id: viewer-sky-irradiance-ignores-local-probes
title: Sky-lit faces take their probe ambient from the view's probe, never from a placed probe
topic: viewer
status: bugs
origin: viewer-realtime-mirrors (2026-11-10)
refs: [viewer-realtime-mirrors, viewer-legacy-material-exact-port]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

`sky_probe_irradiance` (`sl-client-bevy/src/sky_lighting.wgsl`), the diffuse
ambient every sky-lit prim face is lit by, samples only the view's own
(default) probe (`light_probes.view_cubemap_index`). A face standing inside a
placed reflection probe's volume therefore takes the outdoor sky's ambient
rather than what that probe captured — a room lit by its own probe stays lit
like the open sky.

The reference's `sampleReflectionProbes` / `sampleReflectionProbesLegacy`
take the ambient (`ambenv`) from the probes that hold the fragment, the way
they take the reflection.

The specular half had the same gap and was fixed for
[[viewer-realtime-mirrors]]: `sky_probe_radiance` now goes through Bevy's
per-fragment `compute_radiances`. The irradiance can follow the same way
(`compute_radiances` returns both), but it changes the look of every face
inside a probe and wants a before/after look, so it is its own task.

The legacy (Blinn-Phong) branch's environment reflection is
[[viewer-legacy-material-exact-port]]'s.
