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
