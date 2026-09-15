---
id: viewer-sky-sunset-preset-glow-divergence
title: The legacy sunset sky is far brighter, with a much larger glow, than
  Firestorm's
topic: viewer
status: bugs
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
