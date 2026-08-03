---
id: viewer-sun-disc-grey-aditi-hdr-scale
title: Sun disc renders grey on aditi (EEP sky needs sky_hdr_scale)
topic: viewer
status: bugs
origin: viewer-clouds-sun-occlusion-horizon-contact investigation (2026-08-03)
refs: [viewer-clouds-sun-occlusion-horizon-contact]
---

Context: [context/viewer.md](../context/viewer.md).

On **aditi** the sun **disc** renders as a flat grey circle, darker than the
surrounding bright sky (a user report during the sky-colour / bloom work),
whereas Firestorm's sun is a bright, bloomed orb. The disc's own shader
(`sun_disc.wgsl`) is a faithful port of `sunDiscF.glsl` and the draw order is
correct — the disc is simply not bright enough to blow out.

**Suspected cause:** the reference scales all WL-sky pixels (including the disc)
by `sky_hdr_scale` before tone-mapping. For **legacy / classic-mode** skies that
factor is `1.0` (the shipped default, `RenderSkyAutoAdjustLegacy = false`),
which is what the 2026-08-03 sky-colour fix assumes. But an **EEP** sky with a
non-zero `reflection_probe_ambiance` sets `sky_hdr_scale = sqrt(ambiance) * 2`
(> 1), which pushes the disc above 1.0 so it blows out (and, with bloom,
haloes). We do **not** decode `reflection_probe_ambiance` (it is not on
`SkySettings`), so we always use `1.0`, leaving the disc capped ~grey on aditi
EEP skies.

**Work:**

- Decode the sky's `reflection_probe_ambiance` into `SkySettings` (sl-proto).
- Compute `sky_hdr_scale` per the reference (`llsettingsvo.cpp`: probe-ambiance
  → `sqrt(g)*2`; legacy classic-mode → `1.0`; auto-adjust →
  `RenderSkyAutoAdjustHDRScale`) and apply it in the sky / cloud / **sun-disc**
  shaders (a new uniform, alongside the `SL_VIEWER_SKY_LINEARIZE` knob).
- Verify on **aditi** (the disc does not render on local OpenSim — its texture
  404s there), with a Firestorm side-by-side.

Not reproducible on the local grid; needs an aditi login.
