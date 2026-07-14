---
id: viewer-p33-3
title: Reflection-probe brightness calibration
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 33 — Reflection probes
---

Context: [context/viewer.md](../context/viewer.md).

**P33.3. Reflection-probe brightness calibration.** Calibrate the probe's
reflection / ambient contribution against the viewer's mixed material /
exposure model (custom sky / terrain / water vs `StandardMaterial`).

A probe is calibrated when it **reproduces** the surroundings it captured
rather than re-scaling them — a mirror shows the world at the radiance the eye
sees it, and a diffuse surface's ambient is the irradiance that world casts
(the reference never rescales a probe's radiance either: `radscale` is 1 in
`LLReflectionMapManager::updateUniforms`). That is one equation,
`intensity * exposure == gain` with `gain == 1`, so `probe_intensity` is now
**derived from the view's `Exposure`** and P33.1's hand-tuned `1200` is gone;
`SL_VIEWER_PROBE_GAIN` remains only as an A/B knob. The same product is what
the custom terrain / water shaders form when they sample the probe, so both
material families land on the one gain.

The equation only *closes*, though, if the eye and the capture see the same
scene — and they did not. The camera rendered to an **8-bit** target, which is
Bevy's cue to tonemap `StandardMaterial` inside the mesh shader while the
custom sky / terrain / water materials (which never call Bevy's tonemapper)
were merely **clipped at 1.0**. Two visible symptoms, one cause: the terrain
read a different colour in the mirror ball (tone-mapped, as a PBR surface) than
in the main view (raw); and — since the probes' capture cameras are HDR and
un-tonemapped — a probe's cubemap held the sky at its true radiance (the sky
shader ends in the reference's `clamp(color, 0, 5)`) where the eye saw it
flattened to white, so the probes lit the world several times too brightly, an
over-bright sky-blue terrain ambient that no constant `intensity` could have
corrected. So the calibration also:

- gives the main camera an **HDR** target (`Hdr` + an explicit `Exposure`, with
  Bevy's `Tonemapping::None`), putting every material in the one linear space
  the probes capture; and
- adds the **reference viewer's own tone mapper** as the single transfer at the
  end of the frame (`tonemap.rs` / `tonemap.wgsl`, after the underwater fog as
  in the reference): `postDeferredTonemap.glsl` / `tonemapUtilF.glsl`'s
  `toneMap` — `RenderExposure`, then `RenderTonemapType` (0 = Khronos PBR
  Neutral, 1 = ACES Hill, the default), blended back toward the exposed linear
  colour by `RenderTonemapMix` (0.7), then clamped. Bevy's own curves have no
  mix and no Khronos Neutral, so content authored for the reference needs the
  reference's curve. Knobs: `SL_VIEWER_TONEMAP` (`aces` / `neutral` / `none`),
  `SL_VIEWER_TONEMAP_MIX`, `SL_VIEWER_EXPOSURE`.

Not modelled: a probe's **ambiance** (which in the reference scales only the
irradiance half and blends the flat sky ambient back in below 1). Bevy's probe
has a *single* `intensity` across both halves, so the irradiance cannot be
scaled without dragging the reflection with it, and the reflection must stay at
unit gain; every probe therefore runs at the reference's **ambiance-1** point,
where the probe's irradiance *is* the ambient and no flat fill is added — which
is what `suppress_global_ambient` already arranges. The reference's
**automatic** exposure (its luminance-driven `exposureMap` scaling
`RenderExposure`) is not ported — see
[viewer-tonemap-auto-exposure](../ideas/viewer-tonemap-auto-exposure.md).
