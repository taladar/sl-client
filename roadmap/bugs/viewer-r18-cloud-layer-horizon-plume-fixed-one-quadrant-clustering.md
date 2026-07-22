---
id: viewer-r18
title: Cloud layer — horizon plume fixed, one-quadrant clustering still broken
topic: viewer
status: bugs
origin: VIEWER_ROADMAP.md — Known rendering issues (to fix)
---

Context: [context/viewer.md](../context/viewer.md).

**R18. Cloud layer — horizon plume fixed, one-quadrant clustering still
broken** (`sl-client-bevy` / `sl-client-bevy-viewer`, P22.4). Noticed while
verifying P23.1 water. Two distinct defects, one fixed, one **still open**:

- **(fixed) Vertical horizon plume.** The old port evaluated the cloud UV
  *per fragment* from the view direction over a **full sphere**; near the
  horizon that projection is degenerate (`base_uv ∝ (1−cos elev)`, quadratic),
  smearing the texture into a vertical plume. **Fix:** render clouds on a
  CPU replica of the reference `LLVOWLSky` dome — the `calcPhi` zenith cap
  (φ∈[0,π/8]) with the reference **baked** planar texcoords
  (`buildStripsBuffer`, `((-z0+1)/2,(-x0+1)/2)`), and the camera-height offset
  (`DOME_OFFSET × DOME_RADIUS` = `0.96×15000`) baked into the vertices so the
  shallow cap wraps down to fill the sky (the reference puts the camera high
  inside the dome). `clouds.wgsl` now samples the interpolated vertex UV.
- **(still open) Clouds cluster into ~one quadrant** with the other three
  near empty — on BOTH grids, not faithful (Firestorm spreads them evenly,
  reaching the horizon). Verified every checkable element of the port matches
  `class1/deferred/cloudsV` (the φ∈[0,π/8] dome, `calcPhi`, baked UV,
  `cloud_scale=0.4199`, the `0.96×15000` offset, the repeat sampler,
  `drawDome`→`mStripsVerts`) — there is only one cloud shader (no `class2`/
  `class3` variant; `LLVOClouds` gone), so the code path is right. NOTE: a
  **separate confound was ruled out** — the EEP environment was not ingested
  on aditi at all (see R19), so aditi ran on WindLight defaults; with R19
  fixed aditi now loads its real EEP and still shows the one-quadrant
  clustering. The `SL_VIEWER_LOG_CLOUDS` env var logs the live cloud EEP
  params + resolved texture id for comparison.

**Root cause (found 2026-07-22, not yet fixed): the cloud-noise texture is
uploaded sRGB; the reference samples it raw.**

- `apply_cloud_textures` (`sl-client-bevy-viewer/src/sky.rs`) uploads the
  decoded noise via `to_bevy_image` (`sl-client-bevy/src/textures.rs`),
  which uses `TextureFormat::Rgba8UnormSrgb` — the GPU sRGB→linear-decodes
  every sample (mid-gray byte 128 → 0.216). `clouds.wgsl` then treats the
  sample as raw data (`cloudNoise(uv).x - 0.5` is the density term,
  faithful to `cloudsF.glsl`).
- The reference binds the noise as a plain `GL_RGBA8` texture (Firestorm
  `llimagegl.cpp` default for 4-component fetched textures; `llvosky.cpp`
  even calls `setExplicitFormat(GL_RGBA8, GL_RGBA)`) and `cloudsF.glsl` has
  no `srgb_to_linear` — the reference shader sees the raw byte values.
- Quantified on the actual default cloud texture
  (`1dc1368f-e8fe-f02d-a08d-9d9f11c1af6b`, reassembled from the viewer's
  own texture cache — 600-byte head in `texture.cache` + `.texture` body —
  and decoded with `opj_decompress`): the `alpha1 > 0` threshold (linear
  ≈0.46 with default `cloud_shadow` 0.27) needs byte ≥ ~117 raw but
  byte ≥ ~181 after sRGB decode. Texels qualifying: **~46% raw vs ~9%
  sRGB-decoded**. Spatially (4×4 block coverage map of the texture): at
  ≥117 every block has 17–88% coverage (clouds everywhere); at ≥181 most
  blocks are 0–5% and the survivors concentrate in a few isolated blobs —
  exactly "clouds in one region, rest empty" within the ~0.9-tile window
  the dome projects. The disturbance octaves and the `alpha2` self-shadow
  are skewed the same way (milder).
- **Projection deviation ruled out** (do not re-investigate): the
  reference's dome draw sets the shader uniform
  `camPosLocal = (0, camHeightLocal, 0)` in dome space
  (`lldrawpoolwlsky.cpp` `renderDome`), so its
  `rel_pos = position − (0, 14400, 0) + (0, 50, 0)` is exactly the
  offset-baked local position the port uses; the reference also hits the
  same `altitude_blend_factor` ≈0.32 at the horizon and the droop clamp
  below it. The port's projection/lighting geometry are faithful; the
  earlier "altitude-plane projection" candidate is unnecessary.
- **Fix direction:** upload the cloud noise linear (`Rgba8Unorm`) + repeat,
  like the four existing linear uploaders (`bump.rs`,
  `legacy_materials.rs`, `water.rs`, `materials.rs`) — the
  `to_bevy_image`-is-sRGB-only trap already documented for normal maps. The
  redundant repeat-sampler override in `apply_cloud_textures` can fold into
  that uploader.
