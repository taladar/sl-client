---
id: viewer-pbr-face-sky-lighting-divergence
title: PBR (glTF) faces are still lit by Bevy's model, not the reference's
topic: viewer
status: bugs
origin: split off viewer-sunlit-face-clips-two-channels (2026-09-16)
refs: [viewer-sunlit-face-clips-two-channels]
---

Context: [context/viewer.md](../context/viewer.md).

[[viewer-sunlit-face-clips-two-channels]] moved every **non-PBR** surface
(prim, mesh and sculpt faces, avatars, trees, terrain) onto a port of the
reference's deferred `softenLight` legacy branch, fed by the shared
sky-lighting texture (`sl_client_bevy::sky_lighting`). A face whose extension
is `SL_FACE_MODE_PBR` — a glTF material override — still goes through
`apply_pbr_lighting`, lit by a 10 000 lux Bevy sun tinted with
`ResolvedSky::diffuse`, the probes' image-based light and Bevy's ambient.

The reference's PBR branch is not that either. `pbrBaseLight`
(`class1/deferred/deferredUtil.glsl`) has two combines:

- **classic sky**: an explicit "reconstruct the Blinn-Phong look" path —
  `irradiance = srgb_to_linear(irradiance * 0.9)`,
  `sun_contrib = srgb_to_linear(linear_to_srgb(min(pow(nl, 1.2), scol)) *
  sunlit * 0.7) * PI`, recombined in gamma space with a `1.1` on the sun;
- **EEP sky**: `clamp(nl * (diffPunc + specPunc), 0, 10) * sunlit * 3.0 * scol`
  plus the probe IBL.

Both take the same `sunlit`, `amblit` and probe inputs the legacy port already
carries, so the texture is enough; the work is the shader branch and the
`pbrPunctual` / `pbrIbl` terms.

## Before starting

The catalogue's `pbr-box` cannot measure this yet. At
`sl-crosscheck --look-at pbr-box --look-from 3 --look-above 4
--day-position 0.35` Firestorm draws its glTF top face a flat grey
(`118,118,139`) where ours draws the checker base colour (`201,188,57`), so
the two viewers do not agree on the *material* before lighting is in question.
The same frame shows Firestorm drawing `legacy-material-box`'s top pink/cyan
and not drawing `sculpt-sphere` at all. Settle which side resolves those
fixtures wrongly first, or the comparison measures the fixture.
