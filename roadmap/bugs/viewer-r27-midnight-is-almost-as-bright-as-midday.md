---
id: viewer-r27
title: Midnight is almost as bright as midday — the moon is a half-strength sun
topic: viewer
status: bugs
origin: found by the viewer-render-scene-coverage sky scenes (2026-07), on the first look at sky-midnight beside sky-midday
refs: [viewer-render-scene-coverage]
---

Context: [context/viewer.md](../context/viewer.md).

**R27. Midnight is almost as bright as midday.** With the moon up, the viewer's
scene light is roughly **half** the daytime sun's rather than a night's. Seen
first in the gallery, stepping `sky-midday` → `sky-midnight` and finding the
ground barely darker.

## The premise, and why it does not hold

`crate::sky`'s `SCENE_LIGHT_ILLUMINANCE` says it outright:

> The scene directional light's illuminance (lux). Held constant; the sky's
> computed sun / moon diffuse colour carries the day↔night brightness change
> (a night moon diffuse is a fraction of the daytime sun diffuse), so the light
> dims naturally as the colour darkens without re-scaling the illuminance.

The parenthesis is the assumption, and it is only true of a **setting sun**. The
attenuation term is `exp(-light_atten * lighty)` where `lighty = 1/|light_up|`
of the **active** body — so as the sun drops toward the horizon `lighty` grows,
the exponential collapses, and the colour really does go to nothing. That is
what makes `sky-sunrise` / `sky-sunset` correctly dim.

It is **not** true of a high moon. At midnight the active body is the moon, and
a moon 65° up has `lighty ≈ 1.10` — barely more attenuated than a midday sun's
`1.015`. So the only thing separating night from day is `moon_brightness`, whose
default is `0.5`. Measured, from `resolve_sky` on the legacy WindLight default:

| time | active body | `light_dir` | diffuse |
| --- | --- | --- | --- |
| sunrise | sun, 3° up | `(1.00, 0.05, 0)` | `[0.174, 0.096, 0.040]` |
| midday | sun, 80° up | `(0.17, 0.98, 0)` | `[0.589, 0.587, 0.611]` |
| sunset | sun, 3° up | `(-1.00, 0.05, 0)` | `[0.174, 0.096, 0.040]` |
| **midnight** | **moon, 65° up** | `(0.42, 0.91, 0)` | **`[0.293, 0.291, 0.302]`** |

Midnight is `0.5 ×` midday, and at a constant 10 000 lux that is a half-strength
sun. The reddening at sunrise/sunset, by contrast, is correct and faithful — the
same table is the evidence for both.

## Where the bug is *not*

Worth recording, because the obvious suspect is innocent and re-checking it
costs an hour. **`calculate_light_settings` is a faithful port** of
`LLSettingsSky::calculateLightSettings` (`indra/llinventory/llsettingssky.cpp`),
line for line:

- `getMoonlightColor()` really does `return getSunlightColor(); //moon and sun
  share light color` — sharing the sunlight colour is the reference's behaviour,
  not our shortcut.
- `moon_brightness = getIsMoonUp() ? getMoonBrightness() : 0.001f`, and
  `mMoonDiffuse = componentMult(moonlight, light_transmittance) * moon_brightness`
  — which is exactly what we compute.

So the reference's `mMoonDiffuse` **is** `0.5 × mSunDiffuse` at a comparable
elevation. The divergence is downstream of the colour, in how it becomes light.

## Where it probably is

We map the sky's diffuse onto a Bevy `DirectionalLight` as
*colour × a constant 10 000 lux*, and the reference has no lux at all — its
night is dark because of the deferred/atmospherics path, not because its moon
diffuse is small. `pipeline.cpp` (~6366) is suggestive: it sets `mSunDiffuse`
/`mMoonDiffuse` from `getSunlightColor()` / `getMoonlightColor()` — which are
*equal* — and then normalises each by its own max component, so the pipeline's
two light colours are identical and the day/night difference cannot live there
either. It is in the shading.

So the fix is most likely a **moon illuminance** rather than a colour change:
the one number the reference does not have and we invented. Do not simply scale
`moon_brightness` — that is a sky-frame value the grid sets, and bending it
would wrongly render every region that tunes it.

## How to verify

The R-series method (`context/viewer.md`): a Firestorm side-by-side on the local
OpenSim at a fixed time of day, since only the reference can say how dark a
night should be. The gallery's `sky-midday` / `sky-midnight` pair is the cheap
repro — no login — but it cannot answer "how dark is right".
