---
id: viewer-sky-sunset-preset-glow-divergence
title: The legacy sunset sky is far brighter, with a much larger glow, than
  Firestorm's
topic: viewer
status: done
origin: found A/B-ing [[viewer-clouds-horizon-waterline-contact]]
  (2026-09-15)
refs: [viewer-clouds-horizon-waterline-contact,
  viewer-clouds-sun-occlusion-horizon-contact]
---

Context: [context/viewer.md](../context/viewer.md).

`sl-crosscheck --scenario catalogue --day-position 0.75 --camera-position
4,128,40 --camera-look-at=-2000,128,36`: both viewers report the same sky
(`sky_name` `sky-sunset`, identical sun direction, 4.3° up) in their
`scene.json`, and draw very different skies from it.

- **sl-client:** a pale blue zenith, orange only in a band near the horizon,
  and a white glow around the sun roughly 600 px across at 1920×1080.
- **Firestorm:** a dim grey-blue zenith grading to a dusky pink horizon, and a
  compact glow about 250 px across.

The sun **disc** is not the difference — neither viewer draws one for this
preset since the nil-`sun_id` fix in the same investigation.

The earlier colour work ([[viewer-clouds-sun-occlusion-horizon-contact]])
matched noon, sunrise and sunset against Firestorm on **aditi's EEP** skies.
This is a legacy-WindLight preset on the fake grid, so the suspects are the
legacy path: the `glow` triple (`5.0, 0.001, -0.48` in the default; the
preset's own), the `sun_moon_glow_factor` / haze-glow terms in `sky.wgsl`, or
the legacy-haze defaults and the 8-bit clamp.

## How to verify

Re-run the command above with both viewers and compare the frames; the zenith
colour and the glow's extent should agree.

## ROOT CAUSE (2026-09-16): the reference normalises a legacy sky's light

Both viewers were bound with **identical** uniforms — that much is now
measured, not assumed: both dumps grew an `environment.sky_params` block
(see below) and every one of its 25 fields agreed, `sunlight_color`
`2.8386` included. The shader maths matched too, line for line against
`skyV.glsl` / `skyF.glsl`. And the frames still differed threefold.

What the reference does with that uniform *after* `applySpecial` pushes it
is the answer. The WL sky and cloud programs declare
`mFeatures.calculatesAtmospherics`, so every bind runs
`LLRender::syncLightState` — and **in classic mode that overwrites
`sunlight_color` and `moonlight_color`** (`llrender.cpp`, guarded by
`LLRender::sClassicMode`) with the hardware light's colours, which
`LLPipeline::setupHWLights` has normalised:

```cpp
mSunDiffuse.setVec(psky->getSunlightColor());
F32 max_color = llmax(mSunDiffuse.mV[0], mSunDiffuse.mV[1], mSunDiffuse.mV[2]);
if (max_color > 1.f) { mSunDiffuse *= 1.f / max_color; }
mSunDiffuse.clamp();
```

Linden's `A-6PM` authors `sunlight_color` 2.8386 and `A-6AM` 2.37, so on
those two frames the reference's sky is lit by **1.0**, not by what the
frame says. `A-12PM` (0.8999) and `A-12AM` (0.66) are under the ceiling and
pass through untouched — which is why midday and midnight matched all
along, and why this read as a sunset-only bug rather than as a clamp
nobody had ported. Classic mode is the legacy case: an EEP sky that
authors a `reflection_probe_ambiance` is not in it, and its uniforms are
never overwritten — so the earlier aditi EEP colour work was right to
match, and stays right.

The glow's size follows from the same number. `haze_glow` multiplies
`sunlight` in the haze term, so a sun 2.8× too bright does not just
brighten the glow, it pushes a far wider ring of it past the 8-bit sky
clamp — the 600 px against 250 px.

## FIX

`shader_light_colors` (`sl-viewer-world-scene/src/sky.rs`) is the port: for
a classic-mode sky (`reflection_probe_ambiance == 0`) the sun and moon
colours are scaled so their largest component is 1.0 and then clamped per
component, and both are black when neither body is up ("prevent
underlighting from having neither lightsource facing us"). An EEP sky keeps
its authored value. Both `sky_params` and `cloud_params` use it — the cloud
shader declares `calculatesAtmospherics` too, so the reference overwrites
its light colours in exactly the same way.

The **water** shader's `sunlight_color` is deliberately left alone: it is a
different uniform with a different source (`lldrawpoolwater.cpp` binds the
frame's raw colour as its specular), not the atmospheric one.

Unit tests (`sl-viewer-world-scene`): a legacy sky over the ceiling reaches
the shader normalised; one under it passes through; an EEP sky keeps its
authored value; a sky with neither body up is unlit.

## What the dumps grew

Reading this off pixels cost a day, so neither viewer can hide the block
again: **both** dumps now carry `environment.sky_params` — the sky/cloud
uniform block, 25 fields — and `sl-crosscheck-report` compares it field by
field under the subject `sky_params`. Firestorm's side is
`buildSkyParams` in `fstestscenedump.cpp` on the fork's `test-harness`
branch, read back through the same getters its own uniform pushes use.
A sky divergence is now one run away from naming its own cause.

## Verified

`sl-crosscheck --scenario catalogue --camera-position 4,128,40
--camera-look-at=-2000,128,36` at all four day positions, sampled at four
heights of the frame (sRGB, ours against Firestorm's):

| day | 0.75 sunset | 0.25 sunrise | 0.5 noon | 0.0 midnight |
| --- | --- | --- | --- | --- |
| before | 129,152,178 / 72,85,106 | 98,160,234 / 65,96,158 | matched | matched |
| after | 71,82,102 / 72,85,106 | 63,93,152 / 65,96,158 | matched | matched |

Sunset and sunrise now sit within the same few-1/255 residual midday and
midnight always had, and the glow's extent agrees by eye.

The same frames show one thing that is **not** this: the sun's **specular
streak on the water** is a narrow cool-white line here against a wide warm
orange one there. Filed as [[viewer-water-sun-specular-streak-diverges]].
