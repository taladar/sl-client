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

**Progress (2026-08-03) — `sky_hdr_scale` done; disc grey is broader; harness
built:** `sky_hdr_scale` is implemented faithfully (`reflection_probe_ambiance`
decoded in sl-proto; `sqrt(gamma)*2` for EEP / `1.0` legacy, per
`llsettingsvo.cpp`; applied to the sky / cloud / sun-disc shaders as a new
uniform + an `SL_VIEWER_SKY_HDR_SCALE` A/B override). But live-testing showed
the grey disc is **broader than EEP**: it also reads grey on **legacy** skies
(where `sky_hdr_scale = 1.0`), because the sun texture is a pure-white **LDR**
sprite (linear ~1.0) that alpha-blends *over* the brighter near-sun **HDR** haze
and so reads as a dim hole (on OpenSim the disc texture 404s, so the bug only
shows where the disc actually loads). An additive/max-blend disc hack fixed noon
but broke sunset (over-bloom), so it was reverted to the faithful
`srgb_to_linear(texture) * sky_hdr_scale` alpha-blend — the fix has to be in the
formulas, not the blend.

To fix it against **byte-identical input** to Firestorm, built a **World >
Environment comparison harness**: three groups x four times (Day Cycle = the
region's own EEP frozen per time; Legacy = ported `A-*`; Modern = the real
`KNOWN_SKY_*` EEP library skies Firestorm loads) + Use Shared. Needed a new
`AT_SETTINGS` fetch/decode path: sl-proto `EnvironmentAsset` +
`environment_asset_from_bytes` (LLSD-format-detecting), and the viewer
`EnvironmentAssetManager` (fetch by UUID over `ViewerAsset`, decode, cache;
mirrors `AnimationManager`). Live-validated on aditi: Modern fetch/decode works
(sunset asset swaps in at the horizon, matching Legacy); Day Cycle is faithful
(region cycle at 0.25/0.75 puts the sun mid-altitude, not the horizon — the
region's authored cycle, not a bug).

**Remaining:** port the reference dynamic exposure (`exposureF` /
`generateExposure` — EEP range `1/hdr_scale..hdr_scale`, the `sky_hdr_scale`
counterweight so EEP/Modern skies don't wash); audit `sky.wgsl` near-sun haze vs
the reference for the **legacy** grey disc (exposure is a no-op on legacy) and
the **disc-above-glow** alignment.
