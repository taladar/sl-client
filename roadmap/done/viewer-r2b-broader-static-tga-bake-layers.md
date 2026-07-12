---
id: viewer-r2b
title: Broader static-TGA bake layers
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Known rendering issues (to fix)
---

Context: [context/viewer.md](../context/viewer.md).

**R2b. Broader static-TGA bake layers.** The client-side bake modelled
only worn-wearable texture layers + a solid skin-tone base; the reference
bakes in static `character/` TGA diffuse layers on every avatar. Added a
`LayerSource::Static` plan source (`sl-bake`) that loads/decodes the TGAs
(`image` crate, viewer side) and composites them: the skin-grain base
(`head_skingrain.tga` / `body_skingrain.tga`, tinted by skin colour, replacing
the flat fill), the skin colour details (`head_color.tga` / `upperbody_color`
/ `lowerbody_color`), the eye sclera (`eyewhite.tga`), and the eyelash-shape
alpha (`head_alpha.tga` — carves the lash surround out of the head bake so the
eyelash mesh, which shares the head material, no longer renders an opaque
quad). The procedural cosmetic / bump layers (shading, highlights, lipstick,
blush, freckles) stay out — they need a per-param colour renderer. Eyelash
rendering is only partly done: the opaque quad is gone, but the thin lashes
need `AlphaMode::Blend` (they fall below the masked-bake cutoff) — folded into
**R5**.
