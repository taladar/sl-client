---
id: viewer-pbr-face-sky-lighting-divergence
title: PBR (glTF) faces are still lit by Bevy's model, not the reference's
topic: viewer
status: done
origin: split off viewer-sunlit-face-clips-two-channels (2026-09-16)
refs:
  - viewer-sunlit-face-clips-two-channels
  - viewer-sculpt-sphere-fixture-divergence
  - viewer-legacy-material-exact-port
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

## Resolved (2026-09-16): glTF faces are lit by the reference's PBR branch

### The fixture first

The flat grey top was **the fixture's fault**, not either viewer's.
`sl_test_assets::gltf_material_asset` wrote its own `{version, type, data}`
envelope as *headerless* binary LLSD. This workspace's decoder reads that;
the reference's `LLSDSerialize::deserialize` does not, and Firestorm logged
`Failed to deserialize material LLSD` and drew the face as a plain white
material. The helper now writes through `sl_material::encode_material_asset`,
the encoder the viewer saves a material to inventory with, and Firestorm draws
the checker.

Four of those warnings remain in every Firestorm run, and they are not ours:
Firestorm probes each of the region's four terrain detail ids as a possible
PBR terrain material, and those ids are textures.

The other two fixtures the frame showed are not about PBR and did not stand in
the way:

- `legacy-material-box`'s pink/cyan top in Firestorm is the environment
  reflection of a shiny legacy material, which this viewer does not draw at
  all under a resolved sky — [[viewer-legacy-material-exact-port]].
- Firestorm draws `sculpt-sphere` as a flat sheet where this viewer draws a
  sphere — filed as [[viewer-sculpt-sphere-fixture-divergence]].

### What changed

- `sky_lighting.wgsl` ports `pbrBaseLight` with `pbrPunctual`, `pbrIbl` and
  `calcDiffuseSpecular`: the classic reconstruction (gamma-space ambient, the
  legacy sun times pi to undo the Lambert divide, recombined with a 1.1) and
  the EEP combine (`nl * (diff + spec)` times `sunlit * 3 * shadow`, plus
  occluded probe irradiance). The probe radiance is sampled from the view's
  specular environment map at `roughness × mips`, the reference's
  `(1 - gloss) * max_probe_lod`. The split-sum BRDF is Karis's analytic fit of
  the integral `genbrdflutF.glsl` tabulates, indexed by perceptual roughness
  as the reference's `BRDF(nv, 1 - perceptualRoughness)` is.
- `face_material.wgsl`: a `SL_FACE_MODE_PBR` face under a resolved sky goes
  through that port, emissive included before `sky_legacy_finish` as the
  reference adds it inside `pbrBaseLight`; local lights are added after, as for
  legacy faces. With no sky it keeps `apply_pbr_lighting`.
- `sky_legacy_irradiance` is `sky_irradiance`: both reference branches take
  the same `ambenv`.
- A `catalogue-eep` scenario (the catalogue under
  `sl_test_assets::environment::eep_sky`) so the EEP branch can be compared at
  all; every other fixture sky is classic.
- Cross-check runs are muted (`SL_VIEWER_CAPTURE_AUDIO`, `--capture-audio` to
  opt back in) in both viewers — the catalogue's sound box looped audibly
  through every run.

### Verified (2026-09-16, both viewers, fake grid)

`sl-crosscheck --look-at pbr-box --look-from 3 --look-above 4`, frame 29,
medians over the checker squares of the glTF top face:

| sky | square | before | after | Firestorm |
| --- | --- | --- | --- | --- |
| classic, day 0.35 | red | `255,59,57` | `244,27,30` | `243,26,30` |
| classic, day 0.35 | green | `64,255,57` | `30,205,30` | `28,204,30` |
| classic, day 0.5 | red | | `250,34,41` | `250,32,41` |
| classic, day 0.5 | green | | `31,251,41` | `29,251,41` |
| classic, day 0.0 | red | | `86,5,13` | `86,4,12` |
| classic, day 0.0 | green | | `4,90,13` | `4,90,12` |
| EEP (`catalogue-eep`) | red | | `157,15,28` | `167,14,27` |
| EEP (`catalogue-eep`) | green | | `13,171,33` | `13,175,31` |

The "before" row is against Firestorm's grey top, so it has no reference
value; the brightness it shows is Bevy's 10 000 lux sun. Whole-frame mean
|diff| after: 0.0190 at day 0.35, 0.0211 at 0.5 and 0.0084 at 0.0 (no
comparable before: Firestorm's half of that frame changed with the fixture).

Residual: under the EEP sky the top face's red runs ~10/255 under the
reference's. The EEP branch leans on the probe's irradiance and radiance, and
the two viewers' probes are not the same capture, so part of that is the probe
rather than the combine.
