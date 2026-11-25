---
id: viewer-stars-render-black-in-daylight
title: The star field draws black specks over the daytime sky
topic: viewer
status: done
origin: viewer-texture-filtering-in-linear-space far-floor cross-check (2026-11-25)
refs: [viewer-texture-filtering-in-linear-space, viewer-stars-add-in-encoded-space]
---

Context: [context/viewer.md](../context/viewer.md).

## Cause and fix

The blend. Bevy's `AlphaMode::Add` is its premultiplied pipeline
(`ONE, ONE_MINUS_SRC_ALPHA`): the PBR shader premultiplies and zeroes the alpha
to make it additive, and a custom shader has to do the same. `stars.wesl`
returned the reference's amplified star alpha (`col.a * factor * 32`, times the
twinkle) as is, so the sky behind each star was scaled by `1 - alpha`, which is
negative, and came out black. At night the sky behind is near black anyway,
which is why it only showed in daylight.

The reference draws the field with `BT_ADD_WITH_ALPHA` (`SRC_ALPHA, ONE`) into
an 8-bit G-buffer target, which clamps the fragment to `[0, 1]` before
blending, so a star adds `rgb * min(alpha, 1)`. `stars.wesl` now returns that
colour with an alpha of zero; the clamp is written out because our target is
HDR float.

The GPU readback test `a_star_adds_light_and_never_darkens_the_sky` draws a
black-texel and a white-texel star quad over a grey clear colour. It fails on
the old shader (the black texel drove the grey to 0) and passes now.

On the same `far-floor` cross-check, sl-client's sky (top third of frames 2 and
20) has no dark specks left and shows the stars as faint light dots, as
Firestorm does. Ours are fainter than the reference's, which is a separate
divergence: [[viewer-stars-add-in-encoded-space]].

The second question below is answered by the reference: it draws the field
in daylight too (its `custom_alpha` is above the `0.001` cut-off), so the
stars should be drawn, faintly.

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
