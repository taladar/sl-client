---
id: viewer-texture-mip-chain-missing
title: Face textures are uploaded without a mip chain
topic: viewer
status: ready
origin: found while closing viewer-texture-anisotropic-filtering-missing (2026-09-16)
refs:
  - viewer-texture-anisotropic-filtering-missing
  - viewer-antialiasing-sharpen-aniso
---

Context: [context/viewer.md](../context/viewer.md).

Every image built from a decoded texture — `build_prim_image` /
`to_bevy_image`, the PBR, legacy normal / specular and bump maps, terrain
detail, avatar bakes — goes through `Image::new`, which makes a single mip
level, and no code in the workspace sets `mip_level_count` or generates a chain.
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
  Cost matters: `build_prim_image` is already budgeted per frame
  (`TextureApplyBudget`), and a full chain is a third more memory.
- Switch the face samplers to a linear `mipmap_filter`.
- Keep the in-place refresh (`refresh_derived_images`, `refresh_lod_image`)
  rebuilding the chain with the image.
- A cross-check pose with a far, oblique textured surface (a long checkered
  floor) to compare against Firestorm.
