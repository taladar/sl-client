---
id: viewer-p14-3
title: Alpha
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 14 — Server-published baked texturing (incl. alpha)
---

Context: [context/viewer.md](../context/viewer.md).

**P14.3. Alpha.** Baked textures carry the alpha wearables composited into
their alpha channel; render body-region materials with `AlphaMode::Blend` (or
`Mask`) so alpha'd regions turn invisible — essential so a worn mesh body's
underlying system body is hidden. Fully-transparent region → hide that part.

**Done.** Each decoded bake is classified once (`classify_bake_alpha` →
`BakeAlpha::{Opaque, Masked, Transparent}`, cached per texture id in
`AvatarBakeMaterials::alpha`): a source with no alpha channel (`components < 4`,
the decoder fills alpha opaque) or an all-opaque alpha is `Opaque`; a mix of
kept and carved pixels is `Masked`; an all-carved alpha is `Transparent`. The
0.5 mask cutoff is shared between the `AlphaMode::Mask` threshold and the u8
classification cutoff (128). `apply_bake_image` now sets each region material's
`alpha_mode` from its bake's class — `Opaque` (the cheap opaque pass, correct
for plain skin) or `Mask(0.5)` (carved pixels discarded). `Mask` rather than
`Blend` deliberately: an avatar body is mostly opaque, so masking keeps it in
the depth-writing opaque pass and dodges transparency-sorting artifacts on the
non-convex body, while still carving alpha'd pixels away. A wholly `Transparent`
region is additionally hidden outright by `apply_avatar_part_visibility` (it now
reads `AvatarBakeMaterials` and unions the alpha-transparent slot into the P13.5
`IMG_USE_BAKED_*` hide) — so a worn mesh body's alpha layer hides the underlying
system body even where no `IMG_USE_BAKED_*` sentinel signalled it. Unit-tested;
no library-surface change (viewer-internal). Live-testable only near an avatar
wearing an alpha layer / mesh body (aditi), so the deterministic classification
is the guarantee.
