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
The stock `settings.xml` default is off, but the GPU feature table turns it on
for every graphics level from Mid up, so a default Firestorm filters
anisotropically.

Anisotropy chooses among mip levels, which face textures carry since
[[viewer-texture-mip-chain-missing]] (2026-09-27). The `far-floor` fake-grid
scene is the pose to judge it by: `sl-crosscheck --scenario far-floor
--camera-position 128,96,28.7 --look-at floor-far-edge`. With mips alone our
far half settles to a flat grey (standard deviation 0 across the band at rows
548–560), where the reference's anisotropic sampling keeps the checker
visible to the horizon (21).

Builds on: the deferred pipeline's final resolve.
