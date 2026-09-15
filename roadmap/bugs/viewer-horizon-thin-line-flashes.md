---
id: viewer-horizon-thin-line-flashes
title: Flashes along a thin line near the horizon
topic: viewer
status: bugs
origin: aditi run while tracing viewer-own-avatar-facing-drifts-idle (2026-09-15)
refs:
  - viewer-clouds-horizon-waterline-contact
  - viewer-underwater-fog-background-flicker
---

Context: [context/viewer.md](../context/viewer.md).

Seen by the user on aditi during an unrelated run (the idle facing trace, on
`bugs2` at `8e274869` plus a logging-only diagnostic): **flashes in a thin
line near the horizon**. The camera was the ordinary third-person follow
camera, the avatar standing or turning in place.

What the run's log says about the scene, and nothing more is known:

- Region environment from the EEP cap: the day cycle
  `3 hr day / 1 hr night #4` (8 sky frames, 1 water frame, day length
  14400 s, offset 57600 s).
- Draw distance 512 m.

Not known yet, and worth pinning down on the next sighting: whether the line is
**at the waterline** (the water plane meeting the sky dome) or at a **land
horizon**, its colour (a bright sky/sun colour, black, or a water colour), and
whether the flashes follow **camera rotation** (a per-pixel depth or precision
fight that moves with the view) or come on their own (a per-frame pass that
alternates, like the background flicker behind the underwater fog did).

## Suspects

- **The water plane's far edge against the sky dome**: a thin band where
  the two meet at grazing angles, where depth precision is worst and the
  water shader's reflection and fog both change fastest.
- **The cloud dome's horizon fade** ([[viewer-clouds-horizon-waterline-contact]]
  is about clouds at the same line, though that report is about geometry, not
  flashing).
- **Distant terrain or region-edge geometry** at the far plane, flickering
  in and out of the frustum or the draw distance.

## Capture next time

A screenshot sequence at a fixed pose, so frames can be diffed, rather than
the live window: `--camera-position` / `--camera-look-at` aimed at the
horizon with `SL_VIEWER_SCREENSHOT_{DELAY,INTERVAL,FRAMES}` set to catch
several consecutive frames. If a flash lands between two otherwise identical
frames, crop that line at full resolution. `SL_VIEWER_DISABLE_UNDERWATER_FOG=1`
is the cheap A/B for the fog pass.
