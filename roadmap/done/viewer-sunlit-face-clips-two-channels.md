---
id: viewer-sunlit-face-clips-two-channels
title: A sunlit opaque face pins red and green rather than showing its texture
topic: viewer
status: done
origin: observed while settling viewer-translucent-top-face-reads-opaque (2026-08-30)
refs:
  - viewer-translucent-top-face-reads-opaque
  - viewer-tonemap-auto-exposure
  - viewer-terrain-ambient-probe-classic-fidelity
  - viewer-pbr-face-sky-lighting-divergence
---

Context: [context/viewer.md](../context/viewer.md).

An **opaque** plywood prim face pointing at the sun pins its red channel across
the *whole* cap and green across most of it. Plywood is a mid-tone wood texture,
so a face wearing it in ordinary daylight should not be running out of range.

Measured on the local grid at a pinned day position
(`SL_VIEWER_SKY_DAY_POSITION=0.35`), over a block of the North Region box's top
cap rather than a few pixels:

| tint | R | G | B | pixels at 255 |
| --- | --- | --- | --- | --- |
| opaque | med 255 | med 255 | med 180 | **R 100 %, G 89 %** |
| 50 % | med 242 | med 200 | med 147 | R 0.2 %, G 0 %, B 0 % |

Note the second row: the prim as it normally stands is **not** blown out — it is
an ordinary plywood tan, and nothing about it is near white. It is the *opaque*
case that runs out of range, and only that case. The name of this task was
"clips to white" at first, which overstated it: the clipped colour is a pale
yellow, since blue still has room.

## Why this is what made a blend unreadable

The gap between those rows is the whole of
[[viewer-translucent-top-face-reads-opaque]] — but the mechanism is **not**
mainly the clipping, and a first draft of this said it was.

Alpha blending happens on **linear radiance**, in the HDR buffer, before the
tone mapper. What you look at is the **display-encoded** value after it. So what
reaches the screen is

```text
T(a·L + (1 - a)·S)      and not      a·T(L) + (1 - a)·T(S)
```

and because `T` is compressive those are far apart when the face `L` is much
brighter than the background `S`. With a Reinhard-ish curve, a face at linear
8.0 over water at 0.1: `T(8.0) = 0.89` and `T(0.1) = 0.09`, so half of each
*appearance* would be `0.49` — while the real result is `T(4.05) = 0.80`. At
half coverage the face keeps four fifths of its opaque appearance. Coverage is
geometric; appearance follows radiance through a curve.

That is physically right and the reference does the same (see below). Clipping
is a second, smaller effect on top of it, and it shows in the **hue**: mixing
with blue-grey water ought to pull the face toward blue, and instead `R - B`
goes from 75 opaque to 95 at half coverage — *warmer*. Blue was free to fall
toward the sea's ~101 (180 → 147, almost exactly half and half) while red was
pinned at the ceiling and could only give up 13. Clipping protected red from the
mix.

So the blend is arithmetically correct and its evidence is drowned: mostly by
the brightness ratio, secondarily by red being pinned. What is left worth fixing
is only the over-range part — a face that did not run out of range would carry
the sea visibly at half coverage, and that report would never have been
ambiguous.

## The reference composites the same way

Checked, so nobody re-opens the compositing half of this:

- The alpha pools draw into `mRT->screen`, allocated `GL_RGBA16F`
  (`pipeline.cpp:970`) — a linear **HDR** target where values pass 1.0 freely.
- `renderFinalize` tone-maps that buffer **afterwards**
  (`tonemap(&mRT->screen, ...)`, `pipeline.cpp:8928`), the alpha pools having
  run in `renderGeomPostDeferred`.

Blend in linear HDR, compress after — the same ordering this viewer has, so the
"a bright face keeps most of its appearance at half coverage" behaviour is the
reference's too and is not a divergence.

Not to be confused with the reference's **8-bit gbuffer** clamp, which is a
different buffer on the sky/cloud path (memory
`sl-client-wl-sky-8bit-gbuffer-clamp`), not the alpha screen target.

## What is actually open

Only this: **is a sunlit plywood face supposed to be that bright?** Ours reaches
roughly twice the display ceiling. If Firestorm's does not, the exposure or the
sunlight scale is hot, and the visible cost is that transparency reads weaker
than it should on every lit surface.

- Compare against Firestorm on the same region at the same pinned day position —
  the only thing that can answer it, and the local grid supports it.
- The scene has its own tone mapper (`crate::tonemap`, P33.3) and a camera
  `Exposure`; see [[viewer-tonemap-auto-exposure]] for what that pass already
  does.
- The memory `sl-client-sky-brightness-is-authored-data` records that the
  frame's `sunlight_color` is authored data, and that night is dark *because the
  data says so* — the same lever in the other direction is the first thing to
  check.

## Not to be confused with

The **glow** pass, which does brighten what it blooms but is not this: with
`SL_VIEWER_DISABLE_GLOW=1` the same face lands within 1–3/255.

## Resolved (2026-09-16): legacy surfaces are lit by the reference's model

Answered by the comparison the task asked for. `sl-crosscheck --scenario
catalogue --look-at plain-box --look-from 3 --look-above 4 --day-position 0.35`:
the plywood box's sunlit top was `255,206,150` here against `196,136,91` in
Firestorm, its sunless side a grey `80,87,78` against a brown `88,59,46`, and
the ground in the box's shadow `74,102,54` against `27,40,16`. So yes: it was
too bright, about 2.4x in linear light, and not only in the sun.

### Why

Not a hot exposure or a sun scale to trim. Prim faces were lit by Bevy's
physically based model — a 10 000 lux directional light tinted with the sky's
diffuse colour, image-based ambient from the probes, a Fresnel specular on
every face — and the reference does none of that for a non-PBR surface. Its
deferred `softenLight` legacy branch is a model of its own, and for a legacy
sky (classic mode) not a linear one:

```glsl
amblit = pow(tmpAmbient, 0.9) * 0.57 * ambientLighting(n, l);  // no probe
sunlit = sunlight * exp(-light_atten / lightnorm.y) * 1.35;
color  = srgb_to_linear(amblit * 0.9
                        + linear_to_srgb(min(pow(n.l, 1.2), shadow))
                          * sunlit * 0.7);
color *= srgb_to_linear(albedo);  // then * 1.1, clamped to 11.2
```

Worked by hand from the uniforms both dumps reported for that frame, that
predicts the sunless side at `92,61,48`; Firestorm drew `88,59,46`. The grey
side here was the probe's green-grey ambient and specular, which the reference
does not apply to a legacy face under a legacy sky at all
(`sampleReflectionProbesLegacy`: `ambenv = amblit` when `classic_mode`).

Found on the way: `SKY_SUNLIGHT_SCALE` was `1.5`, read off the
`LLCachedControl` fallbacks in `applySpecial`. `settings.xml` declares
`RenderSkySunlightScale`, `RenderHDRSkySunlightScale` and
`RenderSkyAmbientScale` all at `1.0`, the first two non-persistent. It also
fed the water's specular.

### What changed

- `sl_client_bevy::sky_lighting` (Rust + `sky_lighting.wgsl`): one shared
  2x1 `Rgba16Float` texture carrying `sunlit`, `amblit`, the probe ambiance
  and the mode (none / EEP / classic), rewritten in place by `drive_sky` only
  when its texels change; and the shader port of the legacy branch — both
  modes, the EEP one taking the probe irradiance faded over `amblit` by the
  ambiance. A texture rather than a uniform because every face material binds
  it by one handle, so a day-cycle step re-prepares no material.
- **Faces** (`face_material.wgsl`): a non-PBR face under a resolved sky is lit
  by the port, with Bevy's point and spot lights added from a local-lights-only
  copy of its loops. New `SL_FACE_MODE_DIFFUSE` is the inert default, so a
  glTF face (`SL_FACE_MODE_PBR`) is the only one left on Bevy's lighting. The
  legacy Blinn-Phong highlight takes the sky's shadowed `sunlit_linear`. With
  no sky (gallery, test scenes) a face renders as before.
- **Terrain** binds the same texture and the same port; the per-region
  `TerrainLighting` uniform and `drive_terrain_lighting` (a sweep over every
  region's material per day-cycle step) are gone. This also does what
  `viewer-terrain-ambient-probe-classic-fidelity` asked — the classic blend —
  and settles its other half the other way: under a classic sky the reference
  adds no probe term to legacy terrain.
- `SKY_SUNLIGHT_SCALE` is `1.0`.

### Verified (2026-09-16, both viewers, fake grid)

Box pose above, frame 2, medians over each region:

| day | region | before | after | Firestorm |
| --- | --- | --- | --- | --- |
| 0.35 | box top | `255,206,150` | `197,136,91` | `196,136,91` |
| 0.35 | box side | `80,87,78` | `93,62,48` | `88,59,46` |
| 0.35 | checker red (top) | `255,73,67` | `236,22,17` | `236,22,19` |
| 0.35 | ground | `89,129,57` | `76,103,41` | `74,103,39` |
| 0.35 | box shadow | `74,102,54` | `30,40,19` | `27,40,16` |
| 0.5 | box top | | `201,168,128` | `201,168,127` |
| 0.5 | box side | | `104,86,61` | `99,81,58` |
| 0.0 | box top | | `69,59,70` | `69,59,70` |
| 0.0 | box side | | `33,29,23` | `32,28,22` |

Whole-frame mean |diff| against Firestorm at 0.35 **0.102 → 0.021**; at 0.5
0.025 and at 0.0 0.013. The water streak pose of
`viewer-water-sun-specular-streak-diverges` (day 0.75, frame 29) went from
0.0145 to 0.0143, and its streak 300 rows below the horizon from `188,133,92`
to `160,115,85` against the reference's `145,105,83`.

Residual: shaded faces run ~5/255 brighter than the reference at noon and in
the morning. No SSAO term is ported (`adjustIrradiance`), which is the one
thing in the branch that darkens exactly those.

### Not done here

PBR (glTF) faces are still lit by Bevy's model. The reference's PBR branch has
a classic-mode reconstruction of its own (`pbrBaseLight`), filed as
[[viewer-pbr-face-sky-lighting-divergence]].
