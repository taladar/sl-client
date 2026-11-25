---
id: viewer-texture-filtering-in-linear-space
title: Colour textures are filtered after sRGB decoding, the reference before
topic: viewer
status: done
origin: viewer-texture-mip-chain-missing far-floor cross-check (2026-09-27)
refs: [viewer-texture-mip-chain-missing, viewer-antialiasing-sharpen-aniso, viewer-stars-render-black-in-daylight]
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

## Decided: match the reference (2026-11-25)

Parity won over linear-light filtering. A world colour texture is now uploaded
`Rgba8Unorm` (`TextureUpload::WORLD_COLOR`, `ColorSpace::EncodedSrgb`), and
every shader that samples one decodes the filtered sample with the shared
`sl_client_bevy::srgb` module (`srgb.wesl`, the reference's `srgb_to_linear`):

- the face material: base colour (re-sampled at the material's own UV and
  decoded, keeping Bevy's alpha), the PBR emissive map, and the legacy specular
  map's RGB;
- terrain, which blends the four encoded detail samples and then decodes the
  mix, as `terrainF.glsl` + `softenLight` do;
- the sun and moon discs, the star bloom, the rainbow and halo, and particles.

`TextureUpload::COLOR` is gone, so every caller had to choose. UI images keep
the GPU decode (`TextureUpload::UI_COLOR`, `Rgba8UnormSrgb`), since `bevy_ui`'s
shader expects a linear sample. A media surface's image is shown both in the
UI's browser view and on a face, so it stays `Bgra8UnormSrgb`. Its face material
sets `MAP_FLAG_BASE_DECODED` (`SlFaceExt::sampler_decoded_base`) so the shader
does not decode it a second time. The edit tool's face-grid cursor is now
`Rgba8Unorm` like every other face texture.

Re-measured the same way, with Firestorm in the same run:

| Rows | sl-client before | sl-client after | reference |
| --- | --- | --- | --- |
| 548–560 (farthest) | 128.0 | 128.0 | 125.0 |
| 565–580 | 136.7 | 127.9 | 117.9 |
| 600–640 (mid distance) | 173.9 | 127.5 | 126.9 |

The "before" column is a fresh run of the previous commit, not the September
numbers. The reference's 565–580 band moves between runs (124.3 in September,
117.9 now), while ours stays at the encoded-byte average.

The GPU readback test `a_world_texture_is_filtered_before_it_is_decoded`
(`render_readback.rs`, stage `TEXTURE_FILTER_STAGE` in the render fixtures)
keeps it: the midpoint of a magnified black/white texel pair has to match a
flat byte-128 texel in the same frame, which holds whatever tonemapping does.

Both cross-check runs also showed the star field drawn as **black** specks over
the daytime sky, identically before and after this change; that is filed as
[[viewer-stars-render-black-in-daylight]].
