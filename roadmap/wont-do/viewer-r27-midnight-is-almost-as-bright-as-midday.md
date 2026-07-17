---
id: viewer-r27
title: Midnight is almost as bright as midday — NOT a viewer bug; the scenes had no night in them
topic: viewer
status: wont-do
origin: filed from the viewer-render-scene-coverage sky scenes (2026-07); withdrawn the same week once the reference's actual mechanism was read rather than guessed at
refs: [viewer-render-scene-coverage]
---

Context: [context/viewer.md](../context/viewer.md).

**Withdrawn. There is no viewer bug here.** Kept rather than deleted because the
symptom is convincing, the wrong diagnosis is the *obvious* one, and anyone who
next looks at the sky at night will land in exactly the same place.

## The symptom, which was real

Stepping the gallery from `sky-midday` to `sky-midnight`, the ground was barely
darker — the scene light measured `0.5 x` midday's. That is not what Second Life
looks like.

## The wrong diagnosis (this file's first version)

That `SCENE_LIGHT_ILLUMINANCE` is held constant while only the light's *colour*
varies, on a premise its own doc states — "the light dims naturally as the
colour darkens" — which holds for a **setting sun** (whose attenuation
`exp(-light_atten / |light_up|)` collapses as it drops) and not for a
**high moon**, whose does not. The moon then differs from the sun only by
`moon_brightness = 0.5`. All of that is *true*, and none of it is the cause.

## What the reference actually does

The night is dark because
**the midnight sky frame's `sunlight_color` is authored dark**. It is content,
not computation. Nothing in the reference computes a night:

- `atmosphericsFuncs.glsl`'s `calcAtmosphericVars` — the function that produces
  the deferred renderer's `sunlit`, i.e. the scene light — is
  `sunlight = (sun_up_factor == 1) ? sunlight_color : moonlight_color;` then
  `sunlight *= exp(-light_atten * above_horizon_factor)`.
  **`moon_brightness` is not in that path at all**, and `moonlight_color` is
  bound (`llsettingsvo.cpp`) from `getMoonlightColor()`, which is literally
  `return getSunlightColor(); //moon and sun share light color`.
- `LLSettingsSky::calculateLightSettings`'s `mMoonDiffuse` — the one place
  `moon_brightness` *is* applied, and the thing our `calculate_light_settings`
  ports — reaches rendering only as the sun/moon **disc** colour
  (`llvosky.cpp:527`). Its `getLightDiffuse()` accessor has **no callers**.
- So the day↔night difference is entirely `sunlight_color` (and `ambient`) per
  frame. Linden's own presets: `A-12PM`'s sunlight is `(0.73, 0.78, 0.90)`;
  `A-12AM`'s is `(0.35, 0.36, 0.66)`, its ambient `(0.20, 0.24, 0.33)` against
  midday's `(1.05, 1.05, 1.05)`, and its `star_brightness` `2.0` against `0.0`.
  `A-12AM`'s `sun_angle` of 4.7124 rad (270°) puts the sun straight down, so the
  diametrically-opposed moon is straight up and *it* is the light — carrying the
  frame's own dark blue.

Our viewer already does all of this correctly. `SkySettings::blend` interpolates
`sunlight_color` between frames, so a real EEP day cycle darkens at night on its
own. The one divergence found — we apply `moon_brightness` where the reference's
shader path does not — makes our night *darker*, not brighter, and is left
alone.

## So where did the bright midnight come from

**The scenes.** They took `SkySettings::legacy_windlight_default` — which is a
**single midday frame** (`sunlight_color` `(0.7342, 0.7815, 0.8999)`, identical
to `A-12PM`; the reference's `LLSettingsSky::defaults()` is the same one frame)
— and moved the sun across it. That is an environment that cannot exist
in-world: one palette with the sun somewhere the palette was never authored for.
There was no night in the data to render.

Fixed in the scenes, not the viewer: `render_scene`'s `SkyPreset` now ports
Linden's four canonical WindLight presets (`A-6AM` / `A-12PM` / `A-6PM` /
`A-12AM`, shipped in `app_settings/windlight/skies/`) through the reference's
own `translateLegacySettings` rules. Measured after:

| scene | diffuse | mean | vs midday |
| --- | --- | --- | --- |
| sunrise | `[0.627, 0.203, 0.035]` | 0.288 | 61% |
| midday | `[0.495, 0.468, 0.451]` | 0.471 | 100% |
| sunset | `[0.759, 0.296, 0.068]` | 0.374 | 79% |
| midnight | `[0.035, 0.036, 0.066]` | **0.046** | **10%** |

Midnight is a tenth of midday and blue; the stars switch themselves on from the
frame's own `star_brightness`.

## The lesson worth keeping

**A fixture that invents an input the grid never sends will produce a bug report
against code that is correct.** The scenes' whole value is that they are the
real thing minus the transport — and a sky frame is *content*, so the moment the
fixture authored one itself it stopped being the real thing. Where a path's
behaviour is carried by data rather than code, the fixture has to port the data,
not approximate it.

Also: `calculate_light_settings` being a faithful port was read as evidence the
bug must be downstream. It was evidence the bug was not in the code at all.
