---
id: viewer-legacy-material-exact-port
title: Legacy Blinn-Phong material — exact port (lightFunc LUT + reflection-probe environment)
topic: viewer
status: deferred
origin: user request (2026-07-25) — track the pixel-exact legacy port separately
refs: [viewer-custom-face-material-shader, viewer-sunlit-face-clips-two-channels]
---

Context: [context/viewer.md](../context/viewer.md).

[[viewer-custom-face-material-shader]] renders the legacy Blinn-Phong material
with a **close approximation**: an analytic normalized Blinn-Phong specular lobe
(`pow(N·H, exp)`, `exp` mapped from glossiness), the specular map × specular
colour, glossiness gated by the normal-map alpha, and an ambient-scaled
environment term — reusing Bevy's lighting. This looks close and behaves
correctly but is not pixel-identical to Firestorm, because:

- SL's specular highlight is a baked **`lightFunc` LUT** indexed by
  `(N·H, glossiness)` (a precomputed normalized Blinn-Phong lobe), not an
  analytic `pow`.
- The environment / gloss reflection samples SL's **reflection-probe** radiance
  maps (`applyGlossEnv` / `applyLegacyEnv`, Firestorm
  `class3/deferred/reflectionProbeF.glsl`), with SL's own fresnel/energy math.
- SL integrates legacy materials through its **own deferred G-buffer + resolve**
  (`softenLightF.glsl`), a different lighting integrator than Bevy's.

**Do (deferred):** for pixel-closer fidelity, port SL's `lightFunc` LUT
generation and feed Bevy reflection probes / environment maps into the
environment + gloss-reflection terms, matching SL's fresnel/energy as closely as
Bevy allows. Bounded by Bevy's lighting integrator, so still not bit-exact.
Deferred because the approximation is a large improvement already and the exact
port is a substantial, lower-value follow-up.

Reference math: Firestorm `class3/deferred/materialF.glsl`,
`softenLightF.glsl:244-268`, `reflectionProbeF.glsl:893-913`.

## Seen in a cross-check (2026-09-16)

The catalogue's `legacy-material-box` (white specular colour, exponent 128,
environment intensity 64, shiny 3) shows the gap plainly. In
`sl-crosscheck --look-at pbr-box --look-from 3 --look-above 4` Firestorm draws
its top a pink/cyan checker — red `86,31,65` and green `16,97,67` under the
`catalogue-eep` sky — where this viewer draws the plain checker (`164,7,16`,
`6,177,20`): the reference adds the sky's reflection, this viewer adds none.

And under a resolved sky it is now *no* environment term at all, not the
approximation above: [[viewer-sunlit-face-clips-two-channels]] moved legacy
faces onto the `softenLight` port and kept only the Blinn-Phong highlight. The
spec-tinted ambient survives only on the no-sky fallback path.
