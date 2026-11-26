---
id: viewer-stars-add-in-encoded-space
title: Daytime stars are fainter than the reference's, which adds them encoded
topic: viewer
status: done
origin: viewer-stars-render-black-in-daylight far-floor cross-check (2026-11-25)
refs: [viewer-stars-render-black-in-daylight, viewer-texture-filtering-in-linear-space, viewer-stars-srgb-linearize]
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

## What was open

- Whether the star pass can reproduce the encoded-space sum without reading the
  destination: for example by drawing the stars into their own target and
  combining them with the sky in a pass that sees both, or by having the sky
  pass draw the stars itself.
- How far the same approximation moves the clouds against the reference, and
  whether one fix covers both.

## Fix (2026-11-26)

Neither a second target nor a merged pass: the passes drawn over the sky
**evaluate the sky themselves**. The atmosphere `skyF.glsl` writes moved out of
`sky.wesl` into a shared module, `sky_colour.wesl` (`sky_encoded_colour`, the
clamped encoded G-buffer value, and `sky_decoded_colour`, `softenLight`'s decode
and `sky_hdr_scale`). `SkyParams` gained the dome's radius, and the star and
cloud materials carry a copy of the dome's inputs (`SkyParams` plus the rainbow
and halo maps), which `drive_stars` and `drive_clouds` mirror from the dome's
material whenever it changes. A fragment evaluates the sky along its view
direction, so it knows the destination without reading it:

- **Stars** add `decode(clamp(sky + texel * coverage)) - decode(sky)` through
  the existing additive blend, which is the reference's sum exactly, and now
  carries `sky_hdr_scale`.
- **Clouds** (yes, one fix covers both) blend premultiplied and hand the blend
  `decode(mix(sky, cloud, alpha)) - (1 - alpha) * decode(sky)`, so what is left
  is the reference's encoded-space mix. `CloudParams` lost `linearize` and
  `sky_hdr_scale`: a cloud pixel is decoded as the sky's.

The GPU readback test `a_star_is_added_to_the_encoded_sky` draws a grey star
over the default sky and reads the frame back encoded: the star must lift the
sky by its texel (0.251). The old blend lifted it by 0.078 and fails.

## Measured

`far-floor` cross-check, frame 2, top third:

| | stars > 25 above a 7-px median | mean lift of those | peak lift |
| --- | --- | --- | --- |
| before | about 10 | | |
| after | 126 | 39 | 83 |
| Firestorm | 98 | 70 | 169 |

The light per star now matches (the summed lift of one star is about 330 grey
levels here against about 300 there), but ours spreads over about 3 x 3 pixels
where Firestorm's sits on 2 x 2, so the peaks are lower. That is anti-aliasing,
not the blend: the viewer renders with 4x MSAA, so a star quad one or two pixels
across covers its pixels partly, where Firestorm (no MSAA, post-process AA only)
covers them fully or not at all.

The seven clouds of the same frame, peak lift and summed lift:

| cloud | 1 | 2 | 3 | 4 | 5 | 6 | 7 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| before | 49 / 340 | 52 / 524 | 54 / 192 | 32 / 263 | 16 / 225 | 23 / 119 | 37 / 148 |
| after | 36 / 224 | 43 / 394 | 44 / 119 | 33 / 229 | 15 / 204 | 16 / 79 | 32 / 122 |
| Firestorm | 37 / 202 | 43 / 312 | 26 / 117 | 30 / 205 | 15 / 186 | 18 / 82 | 33 / 118 |

An EEP sky (`catalogue-eep`) and a night sky (`far-floor` at day position 0.85)
look the same in both viewers, with no artefacts from the new blends.
