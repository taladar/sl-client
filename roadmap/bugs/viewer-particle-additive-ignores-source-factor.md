---
id: viewer-particle-additive-ignores-source-factor
title: Additive particles ignore the system's source blend factor, so transparent texels add light
topic: viewer
status: bugs
origin: found while fogging the particle billboards (viewer-underwater-alpha-fog-remaining-materials, 2026-11-26)
refs: [viewer-underwater-alpha-fog-remaining-materials]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation (from the code, not yet seen on screen)

`sl_viewer_kit::particle_render` reduces a particle system's blend function to
one bit, `ParticleBlend::Additive` when the destination factor is
`LL_PART_BF_ONE`, and draws that variant with the blend state `(One, One)`. The
source factor the system declares is dropped.

The reference keeps both. `LLVOPartGroup` starts from `BF_SOURCE_ALPHA` /
`BF_ONE_MINUS_SOURCE_ALPHA`, takes the system's own pair through
`getBlendFunc`, and `LLDrawPoolAlpha` sets exactly that pair per draw
(`gGL.blendFunc(params.mBlendFuncSrc, params.mBlendFuncDst, …)`,
`lldrawpoolalpha.cpp`). The usual additive system (`PSYS_PART_BF_SOURCE_ALPHA`,
`PSYS_PART_BF_ONE`) therefore adds `rgb * alpha`, not `rgb`.

The fragment shader does not premultiply either: the unlit path returns the
texture times the tint as it is. So on the additive path a texel's colour is
added at full weight however transparent it is. The default soft blob
(`particles.rs`, white with an alpha falloff) would add a solid square of
light. A sprite with a coloured transparent border would add the border too.

## To do

- Confirm it on screen: an additive emitter with the default texture and with a
  sprite that has a transparent border, on the fake grid or OpenSim.
- Carry the system's source factor (and the full destination factor, not one
  bit) into the pipeline key, and map the `LL_PART_BF_*` values to `wgpu` blend
  factors as `LLRender::eBlendFactor` does. Or premultiply in the shader for
  the `SOURCE_ALPHA` source case, if the set of pairs seen in practice is that
  small.
- The water fog `particle.wesl` now applies (`apply_water_fog`, before the
  blend) is weighted by whatever the source factor turns out to be. With the
  source factor honoured, the fog's in-scatter is weighted by coverage, as the
  reference's is.
