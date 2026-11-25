---
id: viewer-stars-render-black-in-daylight
title: The star field draws black specks over the daytime sky
topic: viewer
status: bugs
origin: viewer-texture-filtering-in-linear-space far-floor cross-check (2026-11-25)
refs: [viewer-texture-filtering-in-linear-space]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

In the `far-floor` cross-check (`sl-crosscheck --scenario far-floor
--camera-position 128,96,28.7 --look-at floor-far-edge`, frame 2), the
sl-client frame shows a few dozen small **black** specks scattered over the
blue daytime sky, in the pattern of the star field. Firestorm's frame of the
same run shows the stars as faint **light** dots, or not at all.

It is the same with and without the shader-side sRGB decode of
[[viewer-texture-filtering-in-linear-space]]: a run of the commit before that
change has the specks in the same places.

## To find out

- What the star pass writes there. `stars.wesl` returns the tinted bloom colour
  with an alpha of `col.a * factor * 32`, and an additive blend should never
  darken the sky. A black speck means either the blend is not additive, or the
  value reaching the target is negative or NaN, or a later pass (glow, exposure,
  tonemap) turns an alpha above 1 into darkness.
- Whether `custom_alpha` (the star brightness) should be near zero at this time
  of day. If it is, the stars should not be drawn at all.
