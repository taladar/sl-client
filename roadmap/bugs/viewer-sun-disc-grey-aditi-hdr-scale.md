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

**Progress (2026-08-04) — dynamic exposure DONE; disc is NOT a glow bug;
faithful glow port started:**

- **Dynamic exposure ported** (`exposure.rs` + `exposure.wgsl`, unit-tested,
  clippy-clean): a fullscreen pass grid-samples the composited scene's average
  luminance over the reference central crop and evaluates the `exposureF` curve
  `s = mix(exp_max, exp_min, pow(clamp(L/coeff,0,1),2))` into a 1×1 exposure map
  the tone mapper samples (`final_exposure = RenderExposure · s`). The
  `exp_min/exp_max` range is the `generateExposure` `[1/hdr_scale, hdr_scale]`
  (EEP) / `(1,1)` (legacy) counterweight, computed from the active sky's
  `reflection_probe_ambiance`/`gamma` and published by `drive_sky`. No-fade path
  (`gExposureProgramNoFade`); history smoothing not ported. Env A/B:
  `SL_VIEWER_DISABLE_DYNAMIC_EXPOSURE`, `SL_VIEWER_EXPOSURE_COEFFICIENT`. Needs
  an aditi run to see its effect (EEP-only; a legacy sky is a no-op).
  **NB it does not, on its own, fix the grey disc** — a uniform pre-tone-map
  multiply does not change the disc-vs-sky ratio.
- **`sky.wgsl` near-sun haze audited: it is a faithful port** of `skyV.glsl` /
  `skyF.glsl` (the `haze_glow`, the `color*2`, `clamp(0,5)`, `srgb_to_linear ·
  sky_hdr_scale`), line-for-line — no discrepancy. So the grey disc is not a
  haze-formula bug.
- **The reference sun disc does NOT bloom via the glow pass.** Traced: the disc
  is drawn deferred (`sunDiscF` writes the G-buffer with `SKIP_ATMOS`);
  `softenLightF`'s SKIP_ATMOS branch emits `srgb_to_linear(baseColor)·hdr_scale`
  and sets `frag_color.a = 0.0`, zeroing the disc's glow-mask alpha; the disc is
  `POOL_WL_SKY` not `POOL_GLOW`; and `generateGlow` runs the luminance extract
  at `minLuminance = 9999` (off) — so SL glow is **alpha-mask-driven**, not
  luminance-driven, and neither the sky nor the disc feeds it. The disc's
  on-screen brightness is purely `srgb_to_linear(disc)·sky_hdr_scale` (~1.0 on a
  legacy sky). **So the grey disc is a disc-vs-near-sun-haze brightness /
  tone-map interaction, not a glow problem**, and its root cause (sky-param
  decode vs tone-map vs disc-texture) needs an
  **aditi pixel comparison against Firestorm** to isolate — still OPEN.

**Remaining:**

- Isolate the grey-disc root cause on aditi (params vs tone-map vs disc texture)
  against a Firestorm side-by-side, using the A/B knobs.
- **Faithful glow port** (in progress, user-approved): replace Bevy's mip-chain
  `Bloom` (a tuned approximation whose strength never generalises) with SL's
  alpha-mask separable-Gaussian glow — glow scalar in the scene alpha channel
  (opaque materials write it; alpha-blended materials keep coverage for colour
  but override the alpha blend to `(Zero, One)` so they don't corrupt the mask),
  then extract `rgb·alpha` → `RenderGlowIterations` Gaussian passes
  (`RenderGlowWidth`/`RenderGlowStrength`, the `[.25,.5,.8,1,…]` kernel) →
  additive `scene + glow`. This makes glow-flagged / fullbright / emissive
  in-world content bloom like Firestorm for all sky settings (its own goal, not
  the disc fix).
