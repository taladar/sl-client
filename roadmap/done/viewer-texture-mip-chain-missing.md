---
id: viewer-texture-mip-chain-missing
title: Face textures are uploaded without a mip chain
topic: viewer
status: done
origin: found while closing viewer-texture-anisotropic-filtering-missing (2026-09-16)
refs:
  - viewer-texture-anisotropic-filtering-missing
  - viewer-antialiasing-sharpen-aniso
---

Context: [context/viewer.md](../context/viewer.md).

Every image built from a decoded texture — prim diffuse, the PBR, legacy normal
/ specular and bump maps, terrain detail, avatar bakes, all of them now through
the one `sl_client_bevy::upload_decoded` — goes through `Image::new`, which
makes a single mip level, and no code in the workspace sets `mip_level_count`
or generates a chain.
A face whose texels are smaller than a pixel is therefore sampled from level 0
alone, whatever its sampler says: a distant or oblique texture aliases and
shimmers as the camera moves instead of settling to its average.

The reference builds a chain for every fetched texture: `LLViewerFetchedTexture`
defaults `usemipmaps = true`, and `LLImageGL::setImage` calls
`glGenerateMipmap` after the upload (core profile), sampling with
`TFO_ANISOTROPIC` (trilinear, anisotropic when `RenderAnisotropic` is on).

This is also a precondition of [[viewer-antialiasing-sharpen-aniso]]'s
anisotropic half: wgpu's `anisotropy_clamp` picks among mip levels, so on a
one-level texture it has nothing to choose from.

## To do

- Generate the chain for world textures, CPU-side at build time or on the GPU
  after upload (wgpu has no `glGenerateMipmap`; Bevy leaves it to the asset).
  Cost matters: the prim-diffuse upload is already budgeted per frame
  (`TextureApplyBudget`), and a full chain is a third more memory. One uploader
  means one place to generate it.
- Switch the face samplers to a linear `mipmap_filter`.
- Keep the in-place refresh (`refresh_derived_images`, `refresh_lod_image`)
  rebuilding the chain with the image.
- A cross-check pose with a far, oblique textured surface (a long checkered
  floor) to compare against Firestorm.

## What the reference does (2026-09-27)

Fetched textures are plain `GL_RGBA8` (sRGB decoded in the shader), and
`LLImageGL::setImage` calls `glGenerateMipmap` after the upload. So its levels
are box averages of the **encoded** bytes, and its sampler is trilinear
(`TFO_ANISOTROPIC`).

## Fix

- **The chain is built where the pixels are made.** `sl_texture::mip_chain`
  (`sl-texture/src/mips.rs`) averages 2×2 blocks of the stored bytes down to
  1×1, halving each side rounding down. `DecodedImage` gained
  `mips: Option<MipChain>`, filled by `with_mips()` inside `decode_j2c` and
  `downsample`. Both run on the store's CPU pool, so a store texture's chain
  costs the frame nothing. The store's byte stats count it.
- **One uploader uses it.** `upload_decoded` reuses a decode's chain and builds
  it on the spot for an image made elsewhere. `upload_pixels` (bump maps,
  placeholders, particle patterns) always builds it. The image's
  `mip_level_count` is set, and the data is level 0 followed by the chain.
  `ImageSamplerDescriptor::linear()` already filters between levels.
- **GPU generation was not an option.** Bevy 0.19's `MipGenerationJobs` writes
  through storage textures, which `Rgba8UnormSrgb` cannot be.
- **Bevy fork fix, `bevy_render` `GpuImage::prepare_asset`.** An image
  replaced by one of the same descriptor reuses its GPU texture and wrote only
  level 0 (`write_texture` at the base extent). With a chain, a same-size
  re-decode would have kept stale lower levels. The reuse path now writes every
  level and layer in `data_order`, the same traversal as wgpu's
  `create_texture_with_data`.
- **A universal render-test check**, `mip_violations`, reports any face texture
  whose `mip_level_count` is short of the full chain for its size.
- **Cross-check scene `far-floor`**: one 64 m fullbright black-and-white
  checker slab, quarter-metre cells, landmarks `floor-near-edge` and
  `floor-far-edge`.

Cost of the chain, best of ten in release: 0.07 ms for 256², 0.29 ms for 512²,
1.4 ms for 1024², 6 ms for 2048². That is paid on the decode pool for store
textures, and on the frame thread only for pixels made there.
