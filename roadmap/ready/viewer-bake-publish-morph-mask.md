---
id: viewer-bake-publish-morph-mask
title: Compute the clothing morph mask our published bake carries
topic: viewer
status: ready
origin: the five-component bake fix (2026-09-02)
points: 3
refs: [viewer-p14-5, test-fake-grid-self-avatar-baked-textures-rejected]
---

Context: [context/viewer.md](../context/viewer.md).

A baked avatar texture is five components — `R G B alpha mask` — and the
fifth is the **clothing morph mask**: the plane an observing viewer reads
back to decide how much of each clothing morph applies at each vertex.
`bake_publish.rs` now writes that plane (it has to; a shorter bake is one
no reference viewer can decode at all), but it writes a constant
`255` — "nothing masks the body" — because nothing in this workspace
computes a real one yet.

What that costs: an observer sees our avatar's flared sleeves, pant flare,
long cuffs and loose-body morphs applied at full strength wherever the
worn clothing sets those params, instead of tapering to nothing where the
fabric ends. It is a fidelity gap on *other people's* screens only, and
only for system-layer clothing with a flare morph — mesh clothing does not
use these morphs at all.

The reference builds it in `LLTexLayerSet::gatherMorphMaskAlpha`
(`indra/llappearance/lltexlayer.cpp`): fill the region's mask plane with
`255`, then let each layer in the set subtract its own coverage through
`LLTexLayerInterface::gatherAlphaMasks`, then restore the alpha channel
from the set's alpha masks (`renderAlphaMaskTextures(.., forceClear =
true)`). The consumer is `LLPolyVertexMask::generateMask`, which samples
the **last** component at each morph vertex's UV, divides by 255 and
optionally inverts per the `<morph_masks>` table's `invert` flag.

This workspace already has both ends of that:

- [[viewer-p14-5]] shipped the consuming half — `sl-avatar`'s `masks`
  module (`MorphMasks::from_xml`, `MaskTexture`, `sample_part`,
  `MorphWeights::apply_masked`) — but samples an observed bake's **alpha**
  channel rather than its fifth plane, which is the reference's own
  `num_components - 1` read in disguise. Worth confirming the two agree
  now that the decoder keeps the aux plane in `DecodedImage::aux`.
- The compositor that produces the bake (`composite_own_region`) already
  walks the same layer list the mask has to be gathered from, so the mask
  is a second output of that walk rather than a new pass.

Acceptance: compositing a region whose layers carry an alpha mask
produces a mask plane that is `255` where no layer covers and lower where
one does; a region with no masking layer still produces all-`255`; and the
published bake round-trips that plane through
`sl_texture::encode_baked_avatar_j2c` / `decode_j2c` into
`DecodedImage::aux`. Live confirmation needs an observer watching our
avatar wear flared system-layer clothing on a client-side-baking grid
(OpenSim), which is the same setup [[viewer-p14-5]] could not force.
