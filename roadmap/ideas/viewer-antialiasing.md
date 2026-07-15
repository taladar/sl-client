---
id: viewer-antialiasing
title: Antialiasing & image-sharpness knobs (FXAA / SMAA, CAS, anisotropic)
topic: viewer
status: ideas
origin: render-feature gap analysis vs Firestorm (2026-07)
---

Context: [context/viewer.md](../context/viewer.md).

The resolve-stage image-quality knobs the deferred pipeline needs and we do not
have yet. Grouped because they are all cheap, all live at the same stage (final
image), and none is big enough to warrant its own task.

- **Antialiasing.** MSAA does not work with a deferred renderer, so SL uses
  **post-process AA** — `RenderFSAAType` selects FXAA or SMAA,
  `RenderFSAASamples` the quality tier. Without it, SL's countless high-contrast
  edges (prims against sky, alpha masks) shimmer and crawl. This is the
  substantial piece.
- **CAS sharpening** (`RenderCASSharpness`) — AMD Contrast-Adaptive Sharpening,
  a cheap post pass that recovers the crispness a temporal/post-AA blurs away.
  Pairs naturally with the AA resolve.
- **Anisotropic filtering** (`RenderAnisotropic`) — a texture-sampler setting,
  not a pass, but it belongs to the same "image sharpness" family: without it,
  oblique surfaces (roads, floors, walls seen at a glancing angle) smear. In
  wgpu it is an `anisotropy_clamp` on the sampler — small, high-impact for SL's
  many ground-plane textures.

Scope: an FXAA and an SMAA post pass with a type/quality selector, the CAS pass,
and anisotropic sampling on the world texture samplers, all behind the graphics
settings. Order matters — AA and sharpening run at the end of the post chain,
after tone mapping and any bloom / DoF.

Reference (Firestorm, read-only): the post-AA passes, `RenderFSAAType` /
`RenderFSAASamples` / `RenderCASSharpness` / `RenderAnisotropic`.

Builds on: the deferred pipeline's final resolve.
