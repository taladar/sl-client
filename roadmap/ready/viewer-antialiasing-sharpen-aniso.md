---
id: viewer-antialiasing-sharpen-aniso
title: CAS sharpening + anisotropic texture sampling
topic: viewer
status: ready
origin: render-feature gap analysis vs Firestorm (2026-07); split from viewer-antialiasing
---

Context: [context/viewer.md](../context/viewer.md).

The two cheap image-sharpness knobs that pair with the post-AA resolve
([[viewer-antialiasing-post]]).

- **CAS sharpening** (`RenderCASSharpness`) — AMD Contrast-Adaptive Sharpening,
  a cheap post pass that recovers the crispness a temporal / post-AA resolve
  blurs away. It runs at the end of the post chain, right after the AA resolve.
- **Anisotropic filtering** (`RenderAnisotropic`) — a texture-sampler setting,
  not a pass, but it belongs to the same "image sharpness" family: without it,
  oblique surfaces (roads, floors, walls seen at a glancing angle) smear. In
  wgpu it is an `anisotropy_clamp` on the sampler — small, and high-impact for
  SL's many ground-plane textures.

Scope: the CAS pass and anisotropic sampling on the world texture samplers, both
behind the graphics settings. CAS runs after tone mapping and AA; anisotropy is
set on the samplers the world material pipeline already builds.

Reference (Firestorm, read-only): `RenderCASSharpness` / `RenderAnisotropic`.

Builds on: the deferred pipeline's final resolve.
