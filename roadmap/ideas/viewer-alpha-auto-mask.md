---
id: viewer-alpha-auto-mask
title: Automatic alpha-mask promotion (RenderAutoMaskAlphaDeferred)
topic: viewer
status: ideas
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-perf-bake-alpha-classify-offthread, viewer-r5,
  viewer-preferences-graphics-tab]
---

Context: [context/viewer.md](../context/viewer.md).

The reference can auto-promote alpha-*blended* diffuse faces whose
texture alpha is effectively binary to alpha-*masked* rendering
(`RenderAutoMaskAlphaDeferred`, the graphics-prefs "Render alpha masks"
checkbox). Masked faces write depth, sort correctly, cast and receive
shadows, and are cheaper than blended ones — a significant win on foliage
and mesh clothing whose creators left the material in blend mode.

We render faces per their declared material alpha mode only
([[viewer-r5]]): `AlphaMode::Mask` is used where declared (e.g. trees in
`objects.rs`), but no automatic promotion exists in `materials.rs` /
`face_material.rs`. The building block is already there: the texture
decode task computes an alpha classification
([[viewer-perf-bake-alpha-classify-offthread]] added it for bakes).

Scope: classify decoded diffuse textures (fully-opaque / binary /
gradient alpha), promote qualifying blended faces to masked rendering
with the reference's cutoff, behind a setting the graphics tab can bind;
verify no haloing on gradient-alpha content before enabling by default.

Reference (Firestorm, read-only): `indra/newview/lldrawpoolalpha.cpp`,
`indra/llrender/llgltexture` automask path,
`indra/newview/app_settings/settings.xml` (RenderAutoMaskAlphaDeferred).

Audit 2026-10-15: the code named above has moved and changed shape. Face
alpha is resolved in `sl-viewer-world-objects/src/textures.rs`
(`resolve_texture_alpha_mode`, `TextureAlpha`): an ordinary (non-rigged)
face with an alpha-bearing texture is *already always* alpha-masked at
`FACE_ALPHA_MASK_CUTOFF` (R22d), while a rigged face blends only when the
decode task's `DecodedTexture::min_alpha` shows real transparency. So the
gap is no longer "no promotion" but the classification itself: nothing
distinguishes binary from gradient alpha, so gradient-alpha prim faces mask
where the reference (auto-mask on) would likely keep blending them, and
binary-alpha rigged faces blend where it may mask (verify against the
reference before relying on either). Re-scope around a
binary/gradient classification in the decode task before promoting.
