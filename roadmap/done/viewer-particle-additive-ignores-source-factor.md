---
id: viewer-particle-additive-ignores-source-factor
title: Additive particles ignore the system's source blend factor, so transparent texels add light
topic: viewer
status: done
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

## Done (2026-11-26)

Confirmed from the reference first: `LLVOPartGroup::getBlendFunc` hands the
system's two `LL_PART_BF_*` bytes to `LLDrawPoolAlpha`, which passes them
straight to `gGL.blendFunc` (the enum is numerically `LLRender::eBlendFactor`),
and the glow is a **second draw** of the same geometry with `(ZERO, ONE, ONE,
ONE)` writing `diffuse.a * glow` (`emissiveF.glsl`). The one-bit `ParticleBlend`
could not be fixed in place: the additive path carried the glow in the
fragment's alpha, so no hardware factor could read the coverage from it.

- `ParticleBlend` is now the pair (`source`, `destination`) of
  `ParticleBlendFactor`, all ten wire values mapped to the `wgpu` factor
  `LLRender` uses (the two destination-alpha values `validBlendFunc` refuses
  are still drawn, as the viewer draws them). A byte past the enumeration falls
  back to that side of the default `(SOURCE_ALPHA, ONE_MINUS_SOURCE_ALPHA)`.
  The pair is in the pipeline key; the main draw blends the colour with it and
  leaves the glow mask alone (`(Zero, One)` on alpha).
- A cloud whose particles have any glow is queued a second time, through one
  shared glow pipeline (`PARTICLE_GLOW`, colour writes masked to alpha,
  `(One, One)`), writing the texel's alpha times the glow. So a source-over
  cloud now glows too, which it never did, and an additive cloud's glow is
  weighted by its texel instead of covering the whole quad.
- The fragment discards texels below the reference's `MINIMUM_ALPHA` (0.004)
  exactly when the pair names `SOURCE_ALPHA` on one side, as `LLDrawPoolAlpha`
  keeps its minimum alpha only then.
- The water fog in `particle.wesl` is unchanged and is now weighted by coverage
  through the source factor, as the reference's is.

Confirmed on the GPU rather than by eye, by two readback tests that each fail
against the old code: `an_additive_particle_adds_its_colour_weighted_by_alpha`
(two `(SOURCE_ALPHA, ONE)` white billboards over black, the half-transparent
one adds half the opaque one's light; the old blend gave a ratio of 1) and
`a_particle_glows_by_its_texel_alpha_whatever_its_blend` (over transparent
black, a source-over billboard at half glow leaves 0.5 in the glow mask, where
it left 0, and a half-transparent additive one at full glow leaves 0.5, where
it left 1). Unit tests cover the factor mapping, the fallback, both blend
states, the discard rule and the glow key sharing.
