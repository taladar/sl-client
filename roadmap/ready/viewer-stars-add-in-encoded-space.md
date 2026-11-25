---
id: viewer-stars-add-in-encoded-space
title: Daytime stars are fainter than the reference's, which adds them encoded
topic: viewer
status: ready
origin: viewer-stars-render-black-in-daylight far-floor cross-check (2026-11-25)
refs: [viewer-stars-render-black-in-daylight, viewer-texture-filtering-in-linear-space]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

With the black specks fixed, the `far-floor` cross-check
(`sl-crosscheck --scenario far-floor --camera-position 128,96,28.7 --look-at
floor-far-edge`, frame 2) shows the daytime stars as faint light dots in both
viewers, but ours are much fainter: in the top third of the frame about 10
pixels stand more than 25 grey levels above a 7-pixel median, against about 110
in Firestorm's.

## Why

The reference writes the WindLight sky into the G-buffer as **encoded** sRGB
(`skyF.glsl`), adds each star's **raw** texel times `min(alpha, 1)` on top of
it (`starsF.glsl`, `BT_ADD_WITH_ALPHA`, an 8-bit target), and only then decodes
the sum and scales it by `sky_hdr_scale` (`softenLightF.glsl`, the
`GBUFFER_FLAG_SKIP_ATMOS` branch). Adding in encoded space on a mid-blue sky
lifts a pixel far more than adding the decoded texel in linear light, which is
what `stars.wesl` does. The stars also skip `sky_hdr_scale` here (1.0 for a
legacy sky, so it matters only under an EEP sky with a probe ambiance).

`sky.wesl` and `clouds.wesl` note the same linear-space approximation for the
sky's own elements; the stars are where it shows most, since a star's whole
contribution is the blend.

## To find out

- Whether the star pass can reproduce the encoded-space sum without reading the
  destination: for example by drawing the stars into their own target and
  combining them with the sky in a pass that sees both, or by having the sky
  pass draw the stars itself.
- How far the same approximation moves the clouds against the reference, and
  whether one fix covers both.
