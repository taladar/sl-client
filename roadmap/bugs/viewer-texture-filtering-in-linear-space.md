---
id: viewer-texture-filtering-in-linear-space
title: Colour textures are filtered after sRGB decoding, the reference before
topic: viewer
status: bugs
origin: viewer-texture-mip-chain-missing far-floor cross-check (2026-09-27)
refs: [viewer-texture-mip-chain-missing, viewer-antialiasing-sharpen-aniso]
---

Context: [context/viewer.md](../context/viewer.md).

Our colour textures are `Rgba8UnormSrgb`, so the GPU decodes each texel to
linear **before** it filters: a bilinear or trilinear blend of black and white
is a linear 0.5, which is written back out as about 188. The reference keeps
fetched textures as plain `GL_RGBA8` and decodes sRGB in the shader
(`srgb_to_linear`), **after** the hardware has filtered the encoded bytes: the
same blend is 128.

Measured on the `far-floor` pair (`sl-crosscheck --scenario far-floor
--camera-position 128,96,28.7 --look-at floor-far-edge`, frame 2, red channel,
x 700–1220):

| Rows | sl-client mean | reference mean |
| --- | --- | --- |
| 548–560 (farthest) | 128.0 | 125.7 |
| 565–580 | 135.4 | 124.3 |
| 600–640 (mid distance) | 174.5 | 125.4 |

The farthest band agrees because the mip levels themselves are averaged in the
encoded bytes on both sides, and a level that deep is one flat colour. Where the
sampler blends distinct texels, ours comes out lighter. So would any
high-contrast texture that is magnified or minified, anywhere, so this is not
specific to distance.

## To decide

Matching the reference means sampling colour maps as `Rgba8Unorm` and decoding
in the shader, which touches Bevy's PBR material path (the base-colour sample
and every other sRGB map). Filtering in linear light is arguably the *correct*
result and the reference's the legacy one. Decide whether parity is worth a
shader-side decode before doing anything.
