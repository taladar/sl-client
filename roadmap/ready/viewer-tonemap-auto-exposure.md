---
id: viewer-tonemap-auto-exposure
title: Automatic (luminance-driven) exposure for the tone mapper
topic: viewer
status: ready
origin: split out of viewer-p33-3 (2026-07)
---

Context: [context/viewer.md](../context/viewer.md).

P33.3 ported the reference viewer's tone mapper (`tonemap.rs` /
`tonemap.wgsl`): exposure, then the chosen curve (Khronos PBR Neutral / ACES
Hill), blended by `RenderTonemapMix`, then clamped. What it did **not** port is
the reference's **automatic exposure**: `toneMap` multiplies the static
`RenderExposure` setting by an `exp_scale` read from a one-texel `exposureMap`,
which the reference maintains from the scene's own luminance —
`RenderDynamicExposureCoefficient` / `RenderDynamicExposureMin` /
`RenderDynamicExposureMax`, plus `RenderSkyAutoAdjustLegacy`'s diffuse
luminance adjustment — so the image adapts as the camera moves from a bright
outdoors to a dim interior, the way an eye does.

Without it the viewer's exposure is the static setting alone, so a dark
interior stays dark and a bright sky stays near the clamp. Building it means a
luminance-reduction pass (a mip chain or a compute reduction over the scene
colour), a one-texel exposure target the reduction writes and the tone mapper
samples, and the reference's temporal adaptation (the exposure eases toward the
target rather than snapping, so a camera turn does not flash).

Worth doing once there is content — a scripted interior, a night region — where
the static exposure visibly gives up. Not a prerequisite for anything.

**Progress (2026-08-04) — most of this landed via the sun-disc work**
([viewer-sun-disc-grey-aditi-hdr-scale](../bugs/viewer-sun-disc-grey-aditi-hdr-scale.md)):
`exposure.rs` + `exposure.wgsl` port the reference's dynamic exposure — a
fullscreen pass reduces the composited scene's average luminance (grid-sample
over the reference central crop, standing in for the mip chain) and evaluates
the `exposureF` curve `s = mix(exp_max, exp_min, pow(clamp(L/coeff,0,1),2))`
into a 1×1 exposure map the tone mapper samples
(`final_exposure = RenderExposure · s`), with `RenderDynamicExposureEnabled` /
`RenderDynamicExposureCoefficient` registered. The `exp_min/exp_max` range is
`generateExposure`'s default (`RenderUseExposureSkySettings = false`) path:
`[1/hdr_scale, hdr_scale]` for an EEP `reflection_probe_ambiance` sky, `(1, 1)`
(inert) for a legacy sky — so, like the reference default, adaptation is only
visible on EEP skies.

**Remaining:** the **temporal adaptation** (`gExposureProgram`'s
`USE_LAST_EXPOSURE` ease toward the previous frame's exposure, so a camera turn
does not snap — this port is the no-fade `gExposureProgramNoFade` path); and the
`RenderUseExposureSkySettings` branch (the sky's `getHDROffset` / `getHDRMin` /
`getHDRMax`), which would let a legacy sky adapt too.
